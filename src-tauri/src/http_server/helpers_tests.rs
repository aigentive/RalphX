use super::{
    automation_bridge_finalize_authorized, finalize_proposals_impl, get_task_context_impl,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, Automation,
    AutomationId, AutomationJudgeState, AutomationPlanApprovalMode, AutomationPlanJudgeState,
    AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatConversation, IdeationAnalysisBaseRefKind,
    IdeationSession, InternalStatus, Project, ProjectId, Task, VerificationStatus,
};
use chrono::Utc;

async fn state_with_durable_pipeline_workspace(
    mode: AgentConversationWorkspaceMode,
    label: &str,
) -> (AppState, IdeationSession) {
    let state = AppState::new_sqlite_for_apply_test();
    let working_directory = format!("/tmp/ralphx-tasks-finalize-guard-{label}");
    let project = state
        .project_repo
        .create(Project::new(
            "Tasks finalize guard".to_string(),
            working_directory.clone(),
        ))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(mode));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id,
        mode,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        format!("ralphx/test/tasks-finalize-guard-{label}"),
        working_directory,
    );
    workspace.task_pipeline_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    (state, session)
}

#[tokio::test]
async fn get_task_context_impl_filters_resolved_blockers_and_keeps_active_ones() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let dependent = state
        .task_repo
        .create(Task::new(project_id.clone(), "Dependent".to_string()))
        .await
        .unwrap();

    let mut active_blocker = Task::new(project_id.clone(), "Active Blocker".to_string());
    active_blocker.internal_status = InternalStatus::Executing;
    let active_blocker = state.task_repo.create(active_blocker).await.unwrap();

    let mut merged_blocker = Task::new(project_id, "Merged Blocker".to_string());
    merged_blocker.internal_status = InternalStatus::Merged;
    let merged_blocker = state.task_repo.create(merged_blocker).await.unwrap();

    state
        .task_dependency_repo
        .add_dependency(&dependent.id, &active_blocker.id)
        .await
        .unwrap();
    state
        .task_dependency_repo
        .add_dependency(&dependent.id, &merged_blocker.id)
        .await
        .unwrap();

    let context = get_task_context_impl(&state, &dependent.id).await.unwrap();

    assert_eq!(context.blocked_by.len(), 1);
    assert_eq!(context.blocked_by[0].id, active_blocker.id);
    assert_eq!(context.tier, Some(2));
    assert!(
        context
            .context_hints
            .iter()
            .any(|hint| hint.contains("Active Blocker")),
        "active blockers should still be surfaced in HTTP task context hints"
    );
    assert!(
        !context
            .context_hints
            .iter()
            .any(|hint| hint.contains("Merged Blocker")),
        "resolved blockers must not be emitted as active HTTP context blockers"
    );
}

#[tokio::test]
async fn native_finalize_rejects_durably_owned_tasks_pipeline_without_transient_link() {
    let (state, session) =
        state_with_durable_pipeline_workspace(AgentConversationWorkspaceMode::Tasks, "tasks").await;

    let error = finalize_proposals_impl(&state, session.id.as_str(), false)
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("waiting for the user to choose Start Tasks"));
    assert!(state
        .task_repo
        .get_by_ideation_session(&session.id)
        .await
        .unwrap()
        .is_empty());

    let external_route_error = finalize_proposals_impl(&state, session.id.as_str(), true)
        .await
        .unwrap_err();
    assert!(external_route_error
        .to_string()
        .contains("waiting for the user to choose Start Tasks"));
}

#[tokio::test]
async fn native_finalize_rejects_durable_tasks_pipeline_after_leaving_tasks_mode() {
    let (state, session) =
        state_with_durable_pipeline_workspace(AgentConversationWorkspaceMode::Chat, "chat").await;

    let error = finalize_proposals_impl(&state, session.id.as_str(), false)
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("waiting for the user to choose Start Tasks"));
    assert!(state
        .task_repo
        .get_by_ideation_session(&session.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn automation_bridge_finalize_authority_requires_current_verified_run_and_plan() {
    let state = AppState::new_sqlite_for_apply_test();
    let project_id = ProjectId::new();
    let automation_id = AutomationId::new();
    let run_id = AutomationRunId::new();
    let artifact_id = ArtifactId::new();
    let now = Utc::now();

    state
        .automation_repo
        .create(Automation {
            id: automation_id.clone(),
            project_id: project_id.clone(),
            name: "Ideation bridge".to_string(),
            status: AutomationStatus::Active,
            paused_reason_code: None,
            paused_reason_detail: None,
            goal_prompt: "Build the task graph".to_string(),
            setup_conversation_id: None,
            provider_harness: "claude".to_string(),
            model_id: "sonnet".to_string(),
            logical_effort: None,
            run_mode: "ideation".to_string(),
            base_ref_kind: "project_default".to_string(),
            base_ref: "main".to_string(),
            base_display_name: None,
            base_source_pull_request_json: None,
            goal_items_json: None,
            chain_mode: "merged_base".to_string(),
            completion_signal: "ideation_finalized".to_string(),
            plan_approval_mode: AutomationPlanApprovalMode::Automatic,
            pr_merge_mode: AutomationPrMergeMode::Manual,
            plan_deep_verification: true,
            max_runs: 1,
            max_consecutive_failures: 1,
            first_run_prompt: Some("Author and finalize the plan".to_string()),
            setup_analysis_summary: None,
            spec_artifact_id: None,
            authoring_state_json: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("create automation");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.automation_id = Some(automation_id.clone());
    conversation.automation_run_id = Some(run_id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Ideation));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create automation conversation");

    state
        .automation_run_repo
        .create_run(AutomationRun {
            id: run_id,
            automation_id,
            run_index: 1,
            status: AutomationRunStatus::Running,
            judge_state: AutomationJudgeState::None,
            judge_lease_expires_at: None,
            plan_judge_state: AutomationPlanJudgeState::Done,
            plan_judge_lease_expires_at: None,
            plan_judge_verdict_json: None,
            plan_revision_round: 0,
            plan_reminder_count: 0,
            plan_pending_instructions: None,
            plan_last_parked_artifact_id: Some(artifact_id.to_string()),
            agent_phase_started_at: Some(now),
            conversation_id: Some(conversation.id),
            run_prompt: "Author and finalize the plan".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: "main".to_string(),
            base_from_run_id: None,
            goal_item_id: None,
            branch_name: Some("ralphx/automation-bridge".to_string()),
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
            started_at: Some(now),
            finished_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("create automation run");

    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .plan_artifact_id(artifact_id.clone())
        .verification_status(VerificationStatus::Verified)
        .build();
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .expect("create verified session");

    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project_id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/automation-bridge".to_string(),
        "/tmp/ralphx-automation-bridge".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("create linked workspace");

    let session_id = session.id.to_string();
    let approved_artifact_id = artifact_id.to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO plan_artifact_approvals (
                    session_id, artifact_id, artifact_version, status, approved_at, approved_by
                 ) VALUES (?1, ?2, 1, 'approved', ?3, 'judge')",
                rusqlite::params![session_id, approved_artifact_id, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await
        .expect("approve current plan");

    assert!(automation_bridge_finalize_authorized(&state, &session)
        .await
        .expect("check trusted bridge"));

    let mut unverified = session;
    unverified.verification_status = VerificationStatus::NeedsRevision;
    assert!(!automation_bridge_finalize_authorized(&state, &unverified)
        .await
        .expect("reject unverified bridge"));
}
