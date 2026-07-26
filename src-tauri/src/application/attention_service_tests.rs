use super::*;
use crate::domain::entities::ChatContextType;

use std::sync::Arc;

use chrono::Utc;

use crate::application::{PendingPermissionInfo, QuestionOption};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentWorkspacePrReviewMonitor, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, ChatConversation, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSession, Project, Task,
};
use crate::domain::repositories::{PlanApprovalActor, PlanArtifactApprovalRepository};
use crate::domain::state_machine::Blocker;
use crate::infrastructure::memory::MemoryPlanArtifactApprovalRepository;

#[test]
fn attention_item_helpers_keep_unknown_categories_global_and_unmapped_items_last() {
    use super::attention_service_items::{attention_group, is_in_scope};

    assert!(is_in_scope(
        None,
        Some(&ProjectId::from_string("project-1".to_string()))
    ));
    assert!(is_in_scope(
        Some("project-1"),
        Some(&ProjectId::from_string("project-1".to_string()))
    ));
    assert!(!is_in_scope(
        Some("project-2"),
        Some(&ProjectId::from_string("project-1".to_string()))
    ));
    assert_eq!(attention_group(NotificationCategory::AgentQuestion), 0);
    assert_eq!(attention_group(NotificationCategory::AgentWaiting), 5);
}

#[tokio::test]
async fn notification_context_resolver_resolves_task_permission_before_fallback_context() {
    let state = AppState::new_test();
    let project = create_project(&state, "resolver-task").await;
    let task = Task::new(project.id.clone(), "Review release notes".to_string());
    state.task_repo.create(task.clone()).await.unwrap();
    let resolver = NotificationContextResolver::from_app_state(&state);

    let resolved = resolver
        .resolve_permission_target(Some(task.id.as_str()), Some("missing-conversation"))
        .await
        .unwrap();

    assert_eq!(resolved.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(
        resolved.context_label.as_deref(),
        Some("Review release notes")
    );
    assert_eq!(resolved.target.kind, NotificationTargetKind::Task);
    assert_eq!(resolved.target.task_id.as_deref(), Some(task.id.as_str()));
}

#[tokio::test]
async fn notification_context_resolver_permission_target_validates_trusted_workspace() {
    let state = AppState::new_test();
    let project = create_project(&state, "resolver-trusted-permission").await;
    let task = Task::new(project.id.clone(), "Approve workspace command".to_string());
    state.task_repo.create(task.clone()).await.unwrap();
    let conversation = ChatConversation::new_project(project.id.clone());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            conversation.id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            "ralphx/trusted-permission".to_string(),
            "/tmp/ralphx-trusted-permission".to_string(),
        ))
        .await
        .unwrap();
    let resolver = NotificationContextResolver::from_app_state(&state);
    let conversation_id = conversation.id.to_string();

    let trusted = resolver
        .resolve_permission_target_with_trusted_conversation(
            Some(task.id.as_str()),
            Some("task"),
            Some(task.id.as_str()),
            Some(&conversation_id),
        )
        .await
        .unwrap();
    let rejected = resolver
        .resolve_permission_target_with_trusted_conversation(
            Some(task.id.as_str()),
            Some("task"),
            Some(task.id.as_str()),
            Some("missing-workspace-conversation"),
        )
        .await
        .unwrap();
    let fallback = resolver
        .resolve_permission_target_with_trusted_conversation(
            Some(task.id.as_str()),
            Some("task"),
            Some(task.id.as_str()),
            None,
        )
        .await
        .unwrap();

    assert_eq!(trusted.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(
        trusted.target.kind,
        NotificationTargetKind::AgentConversation
    );
    assert_eq!(
        trusted.target.conversation_id.as_deref(),
        Some(conversation_id.as_str())
    );
    assert_eq!(rejected.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(rejected.target, NotificationTarget::none());
    assert_eq!(fallback.target.kind, NotificationTargetKind::Task);
    assert_eq!(fallback.target.task_id.as_deref(), Some(task.id.as_str()));
}

#[tokio::test]
async fn notification_context_resolver_accepts_only_active_self_keyed_standalone_target() {
    let state = AppState::new_test();

    let mut active = ChatConversation::new_project(ProjectId::from_string(
        "standalone-notification-fixture".to_string(),
    ));
    active.context_type = ChatContextType::Standalone;
    active.context_id = active.id.as_str();
    state
        .chat_conversation_repo
        .create(active.clone())
        .await
        .unwrap();

    let mut non_self_keyed = ChatConversation::new_project(ProjectId::from_string(
        "standalone-notification-fixture".to_string(),
    ));
    non_self_keyed.context_type = ChatContextType::Standalone;
    non_self_keyed.context_id = "different-conversation".to_string();
    let malformed_error = state
        .chat_conversation_repo
        .create(non_self_keyed)
        .await
        .expect_err("repository must reject a non-self-keyed Standalone conversation");
    assert!(malformed_error.to_string().contains("context_id"));

    let mut archived = ChatConversation::new_project(ProjectId::from_string(
        "standalone-notification-fixture".to_string(),
    ));
    archived.context_type = ChatContextType::Standalone;
    archived.context_id = archived.id.as_str();
    state
        .chat_conversation_repo
        .create(archived.clone())
        .await
        .unwrap();
    state
        .chat_conversation_repo
        .archive(&archived.id)
        .await
        .unwrap();

    let resolver = NotificationContextResolver::from_app_state(&state);
    let active_id = active.id.to_string();
    let accepted = resolver
        .resolve_permission_target_with_trusted_conversation(
            None,
            Some("standalone"),
            Some(&active_id),
            Some(&active_id),
        )
        .await
        .unwrap();
    let wrong_context = resolver
        .resolve_permission_target_with_trusted_conversation(
            None,
            Some("delegation"),
            Some(&active_id),
            Some(&active_id),
        )
        .await
        .unwrap();
    let archived_id = archived.id.to_string();
    let inactive = resolver
        .resolve_permission_target_with_trusted_conversation(
            None,
            Some("standalone"),
            Some(&archived_id),
            Some(&archived_id),
        )
        .await
        .unwrap();

    assert_eq!(
        accepted.target.kind,
        NotificationTargetKind::AgentConversation
    );
    assert_eq!(
        accepted.target.conversation_id.as_deref(),
        Some(active_id.as_str())
    );
    assert_eq!(accepted.project_id, None);
    assert_eq!(wrong_context.target, NotificationTarget::none());
    assert_eq!(inactive.target, NotificationTarget::none());
}

#[tokio::test]
async fn notification_context_resolver_returns_none_for_missing_and_unknown_contexts() {
    let state = AppState::new_test();
    let resolver = NotificationContextResolver::from_app_state(&state);

    let missing = resolver
        .resolve_permission_target(None, None)
        .await
        .unwrap();
    let unknown = resolver
        .resolve_context_target("unsupported_context", "missing")
        .await
        .unwrap();
    let absent = resolver
        .resolve_context_target("task", "missing-task")
        .await
        .unwrap();

    for resolved in [missing, unknown, absent] {
        assert_eq!(resolved.target, NotificationTarget::none());
        assert!(resolved.project_id.is_none());
        assert!(resolved.context_label.is_none());
    }
}

#[tokio::test]
async fn notification_context_resolver_resolves_project_ideation_and_delegation_contexts() {
    let state = AppState::new_test();
    let project = create_project(&state, "resolver-contexts").await;
    let mut project_conversation = ChatConversation::new_project(project.id.clone());
    project_conversation.title = Some("Project planning".to_string());
    state
        .chat_conversation_repo
        .create(project_conversation.clone())
        .await
        .unwrap();
    let session = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Launch plan")
        .build();
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();
    let mut ideation_conversation = ChatConversation::new_ideation(session.id.clone());
    ideation_conversation.title = Some("Ideation chat".to_string());
    state
        .chat_conversation_repo
        .create(ideation_conversation.clone())
        .await
        .unwrap();
    let mut delegation = ChatConversation::new_project(project.id.clone());
    delegation.context_type = ChatContextType::Delegation;
    delegation.title = Some("Delegate report".to_string());
    state
        .chat_conversation_repo
        .create(delegation.clone())
        .await
        .unwrap();
    let resolver = NotificationContextResolver::from_app_state(&state);

    let project_target = resolver
        .resolve_conversation_target(&project_conversation.id)
        .await
        .unwrap();
    let ideation_target = resolver
        .resolve_conversation_target(&ideation_conversation.id)
        .await
        .unwrap();
    let delegation_target = resolver
        .resolve_conversation_target(&delegation.id)
        .await
        .unwrap();

    assert_eq!(
        project_target.project_id.as_deref(),
        Some(project.id.as_str())
    );
    assert_eq!(
        project_target.context_label.as_deref(),
        Some("Project planning")
    );
    assert_eq!(
        project_target.project_name.as_deref(),
        Some("resolver-contexts")
    );
    assert_eq!(project_target.context_kind, Some(ChatContextType::Project));
    assert_eq!(
        ideation_target.project_id.as_deref(),
        Some(project.id.as_str())
    );
    assert_eq!(
        ideation_target.context_label.as_deref(),
        Some("Launch plan")
    );
    assert_eq!(
        ideation_target.project_name.as_deref(),
        Some("resolver-contexts")
    );
    assert_eq!(
        ideation_target.context_kind,
        Some(ChatContextType::Ideation)
    );
    assert!(delegation_target.project_id.is_none());
    assert_eq!(
        delegation_target.context_label.as_deref(),
        Some("Delegate report")
    );
}

#[tokio::test]
async fn notification_context_resolver_handles_task_context_variants_and_missing_owners() {
    let state = AppState::new_test();
    let project = create_project(&state, "resolver-task-variants").await;
    let task = Task::new(project.id.clone(), "Shared task owner".to_string());
    state.task_repo.create(task.clone()).await.unwrap();
    let mut conversations = vec![
        ChatConversation::new_task(task.id.clone()),
        ChatConversation::new_task_execution(task.id.clone()),
        ChatConversation::new_task_execution(task.id.clone()),
    ];
    conversations[1].context_type = ChatContextType::Review;
    conversations[2].context_type = ChatContextType::Merge;
    let mut missing_task = ChatConversation::new_task_execution(
        crate::domain::entities::TaskId::from_string("missing-task".to_string()),
    );
    missing_task.context_type = ChatContextType::Review;
    for conversation in conversations.iter().chain(std::iter::once(&missing_task)) {
        state
            .chat_conversation_repo
            .create(conversation.clone())
            .await
            .unwrap();
    }
    let missing_session = ChatConversation::new_ideation(
        crate::domain::entities::IdeationSessionId::from_string("missing-session".to_string()),
    );
    state
        .chat_conversation_repo
        .create(missing_session.clone())
        .await
        .unwrap();
    let resolver = NotificationContextResolver::from_app_state(&state);

    for conversation in conversations {
        let resolved = resolver
            .resolve_conversation_target(&conversation.id)
            .await
            .unwrap();
        assert_eq!(resolved.project_id.as_deref(), Some(project.id.as_str()));
        assert_eq!(resolved.context_label.as_deref(), Some("Shared task owner"));
    }
    let missing_task_target = resolver
        .resolve_conversation_target(&missing_task.id)
        .await
        .unwrap();
    let missing_session_target = resolver
        .resolve_conversation_target(&missing_session.id)
        .await
        .unwrap();

    assert!(missing_task_target.project_id.is_none());
    assert!(missing_task_target.context_label.is_none());
    assert!(missing_session_target.project_id.is_none());
    assert!(missing_session_target.context_label.is_none());
}

#[tokio::test]
async fn notification_context_resolver_uses_session_fallback_and_ownership_predicates() {
    let state = AppState::new_test();
    let project = create_project(&state, "resolver-sessions").await;
    let session = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Fallback plan")
        .build();
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();
    let automation_session = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Automation plan")
        .build();
    state
        .ideation_session_repo
        .create(automation_session.clone())
        .await
        .unwrap();
    let mut automation_conversation = ChatConversation::new_ideation(automation_session.id.clone());
    automation_conversation.automation_run_id = Some(AutomationRunId::from_string("run-owned"));
    state
        .chat_conversation_repo
        .create(automation_conversation)
        .await
        .unwrap();
    let mut implementation_task = Task::new(project.id.clone(), "Implementation".to_string());
    implementation_task.ideation_session_id = Some(session.id.clone());
    state.task_repo.create(implementation_task).await.unwrap();
    let resolver = NotificationContextResolver::from_app_state(&state);

    let fallback = resolver
        .resolve_ideation_session_target(&session)
        .await
        .unwrap();

    assert_eq!(fallback.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(fallback.target, NotificationTarget::none());
    assert_eq!(fallback.context_label.as_deref(), Some("Fallback plan"));
    assert!(resolver
        .session_has_implementation_task(&session)
        .await
        .unwrap());
    assert!(!resolver
        .session_is_automation_owned(&session)
        .await
        .unwrap());
    assert!(resolver
        .session_is_automation_owned(&automation_session)
        .await
        .unwrap());
    assert!(!resolver
        .session_has_implementation_task(&automation_session)
        .await
        .unwrap());
}

#[tokio::test]
async fn notification_context_resolver_prefers_active_workspace_linked_to_plan_session() {
    let state = AppState::new_test();
    let project = create_project(&state, "resolver-workspace-plan").await;
    let session = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Workspace-owned plan")
        .build();
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();
    let conversation = ChatConversation::new_project(project.id.clone());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "ralphx/plan".to_string(),
        "/tmp/ralphx-plan".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let resolved = NotificationContextResolver::from_app_state(&state)
        .resolve_ideation_session_target(&session)
        .await
        .unwrap();

    assert_eq!(
        resolved.target.conversation_id,
        Some(conversation.id.to_string())
    );
    assert_eq!(
        resolved.target.kind,
        NotificationTargetKind::AgentConversation
    );
}

#[tokio::test]
async fn notification_context_resolver_uses_validated_trusted_workspace_not_newest_project_chat() {
    let state = AppState::new_test();
    let project = create_project(&state, "resolver-exact-workspace").await;
    let trusted = ChatConversation::new_project(project.id.clone());
    let newer = ChatConversation::new_project(project.id.clone());
    state
        .chat_conversation_repo
        .create(trusted.clone())
        .await
        .unwrap();
    state.chat_conversation_repo.create(newer).await.unwrap();
    let workspace = AgentConversationWorkspace::new(
        trusted.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "ralphx/exact".to_string(),
        "/tmp/ralphx-exact".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let trusted_id = trusted.id.to_string();
    let resolved = NotificationContextResolver::from_app_state(&state)
        .resolve_context_target_with_trusted_conversation(
            "project",
            project.id.as_str(),
            Some(&trusted_id),
        )
        .await
        .unwrap();

    assert_eq!(resolved.target.conversation_id, Some(trusted_id));
}

#[tokio::test]
async fn notification_context_resolver_rejects_trusted_workspace_from_another_project() {
    let state = AppState::new_test();
    let expected_project = create_project(&state, "resolver-expected-project").await;
    let other_project = create_project(&state, "resolver-other-project").await;
    let conversation = ChatConversation::new_project(other_project.id.clone());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            conversation.id.clone(),
            other_project.id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            "ralphx/other".to_string(),
            "/tmp/ralphx-other".to_string(),
        ))
        .await
        .unwrap();

    let conversation_id = conversation.id.to_string();
    let resolved = NotificationContextResolver::from_app_state(&state)
        .resolve_context_target_with_trusted_conversation(
            "project",
            expected_project.id.as_str(),
            Some(&conversation_id),
        )
        .await
        .unwrap();

    assert_eq!(
        resolved.project_id.as_deref(),
        Some(expected_project.id.as_str())
    );
    assert_eq!(resolved.target, NotificationTarget::none());
}

fn automation(project_id: ProjectId, id: &str, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id,
        name: format!("Automation {id}"),
        status,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Implement the plan".to_string(),
        setup_conversation_id: None,
        provider_harness: "codex".to_string(),
        model_id: "gpt-5.4".to_string(),
        logical_effort: None,
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: "main".to_string(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 3,
        max_consecutive_failures: 2,
        first_run_prompt: None,
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn automation_run(
    automation_id: AutomationId,
    id: &str,
    status: AutomationRunStatus,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id,
        run_index: 1,
        status,
        judge_state: AutomationJudgeState::None,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Run the automation".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: None,
        pr_number: None,
        pr_url: None,
        pr_title: None,
        pr_head_ref_name: None,
        pr_base_ref_name: None,
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: None,
        judge_verdict_json: None,
        judge_model_id: None,
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: None,
        finished_at: None,
        created_at: now,
        updated_at: now,
    }
}

async fn create_project(state: &AppState, name: &str) -> Project {
    let project = Project::new(name.to_string(), format!("/tmp/{name}"));
    state.project_repo.create(project.clone()).await.unwrap();
    project
}

async fn attention_items(state: &AppState, project_id: Option<&ProjectId>) -> Vec<AttentionItem> {
    AttentionService::from_app_state(state)
        .list_attention_items(project_id.map(ProjectId::as_str))
        .await
        .unwrap()
}

fn task_item_category(items: &[AttentionItem], task: &Task) -> Option<NotificationCategory> {
    items
        .iter()
        .find(|item| item.target.task_id.as_deref() == Some(task.id.as_str()))
        .map(|item| item.category)
}

#[tokio::test]
async fn attention_items_include_actionable_task_statuses_and_exclude_non_actionable_ones() {
    let state = AppState::new_test();
    let project = create_project(&state, "tasks").await;
    let cases = [
        (
            InternalStatus::ReviewPassed,
            NotificationCategory::ReviewNeeded,
        ),
        (
            InternalStatus::Escalated,
            NotificationCategory::ReviewEscalated,
        ),
        (InternalStatus::QaFailed, NotificationCategory::QaFailed),
        (
            InternalStatus::MergeConflict,
            NotificationCategory::MergeConflict,
        ),
        (
            InternalStatus::MergeIncomplete,
            NotificationCategory::MergeIncomplete,
        ),
        (InternalStatus::Failed, NotificationCategory::TaskFailed),
    ];

    let mut included = Vec::new();
    for (index, (status, category)) in cases.into_iter().enumerate() {
        let mut task = Task::new(project.id.clone(), format!("included-{index}"));
        task.internal_status = status;
        state.task_repo.create(task.clone()).await.unwrap();
        included.push((task, category));
    }
    let mut human_blocked = Task::new(project.id.clone(), "needs input".to_string());
    human_blocked.internal_status = InternalStatus::Blocked;
    human_blocked.blocked_reason = Some(Blocker::human_input("approve").id);
    state.task_repo.create(human_blocked.clone()).await.unwrap();

    let mut dependency_blocked = Task::new(project.id.clone(), "depends on task".to_string());
    dependency_blocked.internal_status = InternalStatus::Blocked;
    dependency_blocked.blocked_reason = Some(Blocker::new("other-task").id);
    state
        .task_repo
        .create(dependency_blocked.clone())
        .await
        .unwrap();
    let mut pending_review = Task::new(project.id.clone(), "pending review".to_string());
    pending_review.internal_status = InternalStatus::PendingReview;
    state
        .task_repo
        .create(pending_review.clone())
        .await
        .unwrap();
    let mut reviewing = Task::new(project.id.clone(), "reviewing".to_string());
    reviewing.internal_status = InternalStatus::Reviewing;
    state.task_repo.create(reviewing.clone()).await.unwrap();

    let items = attention_items(&state, None).await;
    for (task, category) in included {
        assert_eq!(task_item_category(&items, &task), Some(category));
    }
    assert_eq!(
        task_item_category(&items, &human_blocked),
        Some(NotificationCategory::TaskBlocked)
    );
    assert_eq!(task_item_category(&items, &dependency_blocked), None);
    assert_eq!(task_item_category(&items, &pending_review), None);
    assert_eq!(task_item_category(&items, &reviewing), None);
}

#[tokio::test]
async fn attention_items_resolve_permission_and_question_projects_and_keep_unknown_items_global() {
    let state = AppState::new_test();
    let project = create_project(&state, "requests").await;
    let other_project = create_project(&state, "other").await;
    let task = Task::new(project.id.clone(), "request context".to_string());
    state.task_repo.create(task.clone()).await.unwrap();
    let conversation = ChatConversation::new_task(task.id.clone());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    state
        .permission_state
        .register(PendingPermissionInfo {
            request_id: "project-permission".to_string(),
            tool_name: "Write".to_string(),
            tool_input: serde_json::json!({}),
            context: None,
            agent_type: None,
            task_id: Some(task.id.to_string()),
            context_type: None,
            context_id: None,
            created_at: Utc::now().to_rfc3339(),
        })
        .await;
    state
        .permission_state
        .register(PendingPermissionInfo {
            request_id: "global-permission".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            context: None,
            agent_type: None,
            task_id: None,
            context_type: None,
            context_id: Some(ChatConversationId::new().to_string()),
            created_at: Utc::now().to_rfc3339(),
        })
        .await;
    state
        .question_state
        .register(
            "project-question".to_string(),
            conversation.id.to_string(),
            "Continue?".to_string(),
            None,
            vec![QuestionOption {
                value: "yes".to_string(),
                label: "Yes".to_string(),
                description: None,
            }],
            false,
        )
        .await;
    state
        .question_state
        .register(
            "global-question".to_string(),
            ChatConversationId::new().to_string(),
            "Unknown context?".to_string(),
            None,
            Vec::new(),
            false,
        )
        .await;

    let items = attention_items(&state, Some(&project.id)).await;
    assert!(items.iter().any(|item| {
        item.id == "permission:project-permission"
            && item.project_id.as_deref() == Some(project.id.as_str())
            && item.target.kind == NotificationTargetKind::Task
    }));
    assert!(items.iter().any(|item| {
        item.id == "question:project-question"
            && item.project_id.as_deref() == Some(project.id.as_str())
            && item.target.kind == NotificationTargetKind::AgentConversation
    }));
    for id in ["permission:global-permission", "question:global-question"] {
        assert_eq!(
            items.iter().find(|item| item.id == id).unwrap().project_id,
            None
        );
    }
    let other_items = attention_items(&state, Some(&other_project.id)).await;
    assert!(other_items
        .iter()
        .any(|item| item.id == "permission:global-permission"));
    assert!(other_items
        .iter()
        .any(|item| item.id == "question:global-question"));
    assert!(!other_items
        .iter()
        .any(|item| item.id == "permission:project-permission"));
}

#[tokio::test]
async fn attention_items_exclude_archived_chat_and_workspace_conversation_targets() {
    let state = AppState::new_test();
    let project = create_project(&state, "archived-attention").await;

    let active_conversation = ChatConversation::new_project(project.id.clone());
    state
        .chat_conversation_repo
        .create(active_conversation.clone())
        .await
        .unwrap();
    let archived_chat_conversation = ChatConversation::new_project(project.id.clone());
    state
        .chat_conversation_repo
        .create(archived_chat_conversation.clone())
        .await
        .unwrap();
    state
        .chat_conversation_repo
        .archive(&archived_chat_conversation.id)
        .await
        .unwrap();
    let archived_workspace_conversation = ChatConversation::new_project(project.id.clone());
    state
        .chat_conversation_repo
        .create(archived_workspace_conversation.clone())
        .await
        .unwrap();
    let mut archived_workspace = AgentConversationWorkspace::new(
        archived_workspace_conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test/archived-attention".to_string(),
        "/tmp/ralphx-test-archived-attention".to_string(),
    );
    archived_workspace.status = AgentConversationWorkspaceStatus::Archived;
    state
        .agent_conversation_workspace_repo
        .create_or_update(archived_workspace)
        .await
        .unwrap();

    for (request_id, conversation_id) in [
        ("active-question", active_conversation.id.to_string()),
        (
            "archived-chat-question",
            archived_chat_conversation.id.to_string(),
        ),
        (
            "archived-workspace-question",
            archived_workspace_conversation.id.to_string(),
        ),
    ] {
        state
            .question_state
            .register(
                request_id.to_string(),
                conversation_id,
                "Continue?".to_string(),
                None,
                Vec::new(),
                false,
            )
            .await;
    }

    let items = attention_items(&state, Some(&project.id)).await;
    assert!(items
        .iter()
        .any(|item| item.id == "question:active-question"));
    assert!(!items
        .iter()
        .any(|item| item.id == "question:archived-chat-question"));
    assert!(!items
        .iter()
        .any(|item| item.id == "question:archived-workspace-question"));
}

#[tokio::test]
async fn attention_items_include_actionable_automation_runs_and_pauses_only() {
    let state = AppState::new_test();
    let project = create_project(&state, "automation").await;
    let mut actionable = automation(project.id.clone(), "actionable", AutomationStatus::Paused);
    actionable.paused_reason_code = Some("judge_failed".to_string());
    actionable.paused_reason_detail = Some("Judge needs intervention".to_string());
    state
        .automation_repo
        .create(actionable.clone())
        .await
        .unwrap();
    let mut user_paused = automation(project.id.clone(), "user", AutomationStatus::Paused);
    user_paused.paused_reason_code = Some("user".to_string());
    state
        .automation_repo
        .create(user_paused.clone())
        .await
        .unwrap();
    for reason in [
        "signal_verification_failed",
        "judge_loop_suspected",
        "judge_stopped_unmet",
        "goal_replan_stale",
        "ideation_bridge_verification_failed",
        "ideation_bridge_missing_session",
    ] {
        let mut paused = automation(
            project.id.clone(),
            &format!("actionable-{reason}"),
            AutomationStatus::Paused,
        );
        paused.paused_reason_code = Some(reason.to_string());
        state.automation_repo.create(paused).await.unwrap();
    }
    let awaiting_run = automation_run(
        actionable.id.clone(),
        "awaiting-plan",
        AutomationRunStatus::AwaitingPlanApproval,
    );
    state
        .automation_run_repo
        .create_run(awaiting_run.clone())
        .await
        .unwrap();
    let auto_merge_automation =
        automation(project.id.clone(), "auto-merge", AutomationStatus::Active);
    state
        .automation_repo
        .create(auto_merge_automation.clone())
        .await
        .unwrap();
    let mut auto_merge_warning = automation_run(
        auto_merge_automation.id,
        "auto-merge-warning",
        AutomationRunStatus::Published,
    );
    auto_merge_warning.pr_number = Some(733);
    auto_merge_warning.error_code = Some("auto_merge_enable_failed".to_string());
    auto_merge_warning.error_detail =
        Some("GitHub rejected automatic merge enablement".to_string());
    state
        .automation_run_repo
        .create_run(auto_merge_warning.clone())
        .await
        .unwrap();

    let items = attention_items(&state, None).await;
    assert!(items.iter().any(|item| {
        item.id == format!("automation:{}:paused", actionable.id)
            && item.category == NotificationCategory::AutomationPaused
    }));
    assert!(items.iter().any(|item| {
        item.id == format!("automation-run:{}:plan-approval", awaiting_run.id)
            && item.category == NotificationCategory::AutomationPlanApproval
    }));
    assert!(items.iter().any(|item| {
        item.id == format!("automation-run:{}:auto-merge", auto_merge_warning.id)
            && item.category == NotificationCategory::AutomationRunFailed
            && item.detail.as_deref() == auto_merge_warning.error_detail.as_deref()
    }));
    assert!(!items
        .iter()
        .any(|item| item.id == format!("automation:{}:paused", user_paused.id)));
    for reason in [
        "signal_verification_failed",
        "judge_loop_suspected",
        "judge_stopped_unmet",
        "goal_replan_stale",
        "ideation_bridge_verification_failed",
        "ideation_bridge_missing_session",
    ] {
        assert!(items.iter().any(|item| {
            item.title == format!("Automation paused: Automation actionable-{reason}")
        }));
    }
}

#[tokio::test]
async fn attention_items_include_only_workspace_plans_awaiting_approval() {
    let state = AppState::new_test();
    let project = create_project(&state, "plans").await;
    let eligible = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Eligible plan")
        .plan_artifact_id(crate::domain::entities::ArtifactId::from_string(
            "eligible-plan",
        ))
        .plan_blueprint_artifact_id(crate::domain::entities::ArtifactId::from_string(
            "eligible-plan-blueprint",
        ))
        .build();
    state
        .ideation_session_repo
        .create(eligible.clone())
        .await
        .unwrap();
    let conversation = ChatConversation::new_project(project.id.clone());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test/attention-plan".to_string(),
        "/tmp/ralphx-test-attention-plan".to_string(),
    );
    workspace.linked_ideation_session_id = Some(eligible.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let owned_by_automation = IdeationSession::builder()
        .project_id(project.id.clone())
        .plan_artifact_id(crate::domain::entities::ArtifactId::from_string(
            "automation-plan",
        ))
        .build();
    state
        .ideation_session_repo
        .create(owned_by_automation.clone())
        .await
        .unwrap();
    let mut automation_conversation =
        ChatConversation::new_ideation(owned_by_automation.id.clone());
    automation_conversation.automation_run_id = Some(AutomationRunId::from_string("run-owned"));
    state
        .chat_conversation_repo
        .create(automation_conversation)
        .await
        .unwrap();
    let implemented = IdeationSession::builder()
        .project_id(project.id.clone())
        .plan_artifact_id(crate::domain::entities::ArtifactId::from_string(
            "implemented-plan",
        ))
        .build();
    state
        .ideation_session_repo
        .create(implemented.clone())
        .await
        .unwrap();
    let mut implementation_task = Task::new(project.id.clone(), "implementation".to_string());
    implementation_task.ideation_session_id = Some(implemented.id.clone());
    state.task_repo.create(implementation_task).await.unwrap();

    let items = attention_items(&state, None).await;
    let eligible_item = items
        .iter()
        .find(|item| item.id == format!("plan:{}:approval", eligible.id))
        .expect("eligible plan should require approval");
    assert_eq!(eligible_item.category, NotificationCategory::PlanApproval);
    assert_eq!(
        eligible_item.target.conversation_id,
        Some(conversation.id.to_string())
    );
    assert!(!items
        .iter()
        .any(|item| item.id == format!("plan:{}:approval", owned_by_automation.id)));
    assert!(!items
        .iter()
        .any(|item| item.id == format!("plan:{}:approval", implemented.id)));
}

#[tokio::test]
async fn attention_items_exclude_current_plan_approvals_but_include_stale_approvals() {
    let approval_repo = Arc::new(MemoryPlanArtifactApprovalRepository::new());
    let mut state = AppState::new_test();
    let plan_approval_repo: Arc<dyn PlanArtifactApprovalRepository> = approval_repo.clone();
    state.plan_approval_repo = plan_approval_repo;
    let project = create_project(&state, "plan-approval-artifacts").await;

    let current_artifact_id = crate::domain::entities::ArtifactId::from_string("current-plan");
    let current_approved = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Current approved plan")
        .plan_artifact_id(current_artifact_id.clone())
        .plan_blueprint_artifact_id(crate::domain::entities::ArtifactId::from_string(
            "current-plan-blueprint",
        ))
        .build();
    state
        .ideation_session_repo
        .create(current_approved.clone())
        .await
        .unwrap();
    approval_repo.approve_bundle(
        current_approved.id.clone(),
        current_artifact_id,
        crate::domain::entities::ArtifactId::from_string("current-plan-blueprint"),
        1,
        PlanApprovalActor::User,
    );

    let current_redrafted_artifact_id =
        crate::domain::entities::ArtifactId::from_string("redrafted-current-plan");
    let stale_approved = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Redrafted plan")
        .plan_artifact_id(current_redrafted_artifact_id)
        .plan_blueprint_artifact_id(crate::domain::entities::ArtifactId::from_string(
            "redrafted-current-plan-blueprint",
        ))
        .build();
    state
        .ideation_session_repo
        .create(stale_approved.clone())
        .await
        .unwrap();
    approval_repo.approve(
        stale_approved.id.clone(),
        crate::domain::entities::ArtifactId::from_string("older-plan"),
        1,
        PlanApprovalActor::User,
    );

    let items = attention_items(&state, None).await;
    assert!(!items
        .iter()
        .any(|item| item.id == format!("plan:{}:approval", current_approved.id)));
    assert!(items.iter().any(|item| {
        item.id == format!("plan:{}:approval", stale_approved.id)
            && item.category == NotificationCategory::PlanApproval
    }));
}

#[tokio::test]
async fn attention_items_include_pr_review_monitors_awaiting_user_only() {
    let state = AppState::new_test();
    let project = create_project(&state, "pr-review").await;
    let conversation_id = ChatConversationId::new();
    let mut awaiting = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project.id.clone(),
        42,
        Some("head".to_string()),
    );
    awaiting.monitor_enabled = true;
    awaiting.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(awaiting)
        .await
        .unwrap();
    let mut reviewing = AgentWorkspacePrReviewMonitor::new(
        ChatConversationId::new(),
        project.id.clone(),
        43,
        Some("head".to_string()),
    );
    reviewing.monitor_enabled = true;
    reviewing.status = AgentWorkspacePrReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(reviewing)
        .await
        .unwrap();

    let items = attention_items(&state, None).await;
    assert!(items.iter().any(|item| {
        item.id == format!("pr-review:{conversation_id}")
            && item.category == NotificationCategory::PrReviewAction
    }));
    assert!(!items.iter().any(|item| item.title.contains("PR #43")));
}

#[tokio::test]
async fn attention_items_are_empty_without_actionable_state_and_scope_project_items() {
    let state = AppState::new_test();
    assert!(attention_items(&state, None).await.is_empty());
    let first_project = create_project(&state, "first").await;
    let second_project = create_project(&state, "second").await;
    let mut first_task = Task::new(first_project.id.clone(), "first failed".to_string());
    first_task.internal_status = InternalStatus::Failed;
    state.task_repo.create(first_task.clone()).await.unwrap();
    let mut second_task = Task::new(second_project.id.clone(), "second failed".to_string());
    second_task.internal_status = InternalStatus::Failed;
    state.task_repo.create(second_task.clone()).await.unwrap();

    let items = attention_items(&state, Some(&first_project.id)).await;
    assert_eq!(
        task_item_category(&items, &first_task),
        Some(NotificationCategory::TaskFailed)
    );
    assert_eq!(task_item_category(&items, &second_task), None);
}
