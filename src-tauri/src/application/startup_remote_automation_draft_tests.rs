use chrono::{Duration, Utc};

use super::{startup_background::dispatch_one_remote_automation_draft, AppState};
use crate::domain::entities::{
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationStatus,
    Project, ProjectId, RemoteAutomationDraftRequest, RemoteAutomationDraftRequestStatus,
};

fn automation(id: AutomationId, project_id: ProjectId) -> Automation {
    let now = Utc::now();
    Automation {
        id,
        project_id,
        name: "Existing".into(),
        status: AutomationStatus::Draft,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: String::new(),
        setup_conversation_id: None,
        provider_harness: "claude".into(),
        model_id: "sonnet".into(),
        logical_effort: None,
        run_mode: "edit".into(),
        base_ref_kind: "project_default".into(),
        base_ref: String::new(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".into(),
        completion_signal: "pr_merged".into(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: None,
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn request(
    project_id: &ProjectId,
    automation_id: &AutomationId,
    status: RemoteAutomationDraftRequestStatus,
    claimed_at: Option<chrono::DateTime<Utc>>,
) -> RemoteAutomationDraftRequest {
    let now = Utc::now();
    RemoteAutomationDraftRequest {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.as_str().into(),
        automation_id: automation_id.as_str().into(),
        name: "Existing".into(),
        authoring_mode: "reviewed".into(),
        base_ref_kind: "project_default".into(),
        base_branch_mode: "isolated".into(),
        base_branch: None,
        status,
        error_code: None,
        result: None,
        claimed_at,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn existing_preallocated_automation_completes_without_creating_conversation_and_reentry_is_noop(
) {
    let state = AppState::new_test();
    let project = Project::new("Drafts".into(), "/tmp/drafts".into());
    state.project_repo.create(project.clone()).await.unwrap();
    let automation_id = AutomationId::new();
    state
        .automation_repo
        .create(automation(automation_id.clone(), project.id.clone()))
        .await
        .unwrap();
    let row = state
        .remote_automation_draft_request_repo
        .create_remote_automation_draft_request(request(
            &project.id,
            &automation_id,
            RemoteAutomationDraftRequestStatus::Pending,
            None,
        ))
        .await
        .unwrap();
    assert!(state
        .chat_conversation_repo
        .list_by_automation_id(&automation_id)
        .await
        .unwrap()
        .is_empty());
    dispatch_one_remote_automation_draft(&state).await.unwrap();
    let settled = state
        .remote_automation_draft_request_repo
        .get(&row.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        settled.status,
        RemoteAutomationDraftRequestStatus::Completed
    );
    assert_eq!(
        settled.result.unwrap()["automationId"],
        automation_id.as_str()
    );
    dispatch_one_remote_automation_draft(&state).await.unwrap();
    assert!(state
        .chat_conversation_repo
        .list_by_automation_id(&automation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn stale_starting_request_is_terminal_and_never_dispatched() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("stale-project".into());
    let automation_id = AutomationId::new();
    let row = state
        .remote_automation_draft_request_repo
        .create_remote_automation_draft_request(request(
            &project_id,
            &automation_id,
            RemoteAutomationDraftRequestStatus::Starting,
            Some(Utc::now() - Duration::seconds(301)),
        ))
        .await
        .unwrap();
    state
        .remote_automation_draft_request_repo
        .fail_stale(Utc::now() - Duration::seconds(300), Utc::now())
        .await
        .unwrap();
    dispatch_one_remote_automation_draft(&state).await.unwrap();
    assert_eq!(
        state
            .remote_automation_draft_request_repo
            .get(&row.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteAutomationDraftRequestStatus::FailedStale
    );
    assert!(state
        .automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .is_none());
}
