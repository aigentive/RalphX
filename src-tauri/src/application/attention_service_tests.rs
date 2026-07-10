use super::*;

use std::sync::Arc;

use chrono::Utc;

use crate::application::{PendingPermissionInfo, QuestionOption};
use crate::domain::entities::{
    AgentWorkspacePrReviewMonitor, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, ChatConversationId, IdeationSession,
    Project, Task,
};
use crate::domain::repositories::{PlanApprovalActor, PlanArtifactApprovalRepository};
use crate::domain::state_machine::Blocker;
use crate::infrastructure::memory::MemoryPlanArtifactApprovalRepository;

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
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Run the automation".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
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
            created_at: "2026-07-10T12:00:00Z".to_string(),
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
            created_at: "2026-07-10T13:00:00Z".to_string(),
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

    let items = attention_items(&state, None).await;
    assert!(items.iter().any(|item| {
        item.id == format!("automation:{}:paused", actionable.id)
            && item.category == NotificationCategory::AutomationPaused
    }));
    assert!(items.iter().any(|item| {
        item.id == format!("automation-run:{}:plan-approval", awaiting_run.id)
            && item.category == NotificationCategory::AutomationPlanApproval
    }));
    assert!(!items
        .iter()
        .any(|item| item.id == format!("automation:{}:paused", user_paused.id)));
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
        .build();
    state
        .ideation_session_repo
        .create(eligible.clone())
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
    assert!(items.iter().any(|item| {
        item.id == format!("plan:{}:approval", eligible.id)
            && item.category == NotificationCategory::PlanApproval
    }));
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
        .build();
    state
        .ideation_session_repo
        .create(current_approved.clone())
        .await
        .unwrap();
    approval_repo.approve(
        current_approved.id.clone(),
        current_artifact_id,
        1,
        PlanApprovalActor::User,
    );

    let current_redrafted_artifact_id =
        crate::domain::entities::ArtifactId::from_string("redrafted-current-plan");
    let stale_approved = IdeationSession::builder()
        .project_id(project.id.clone())
        .title("Redrafted plan")
        .plan_artifact_id(current_redrafted_artifact_id)
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
