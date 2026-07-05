use std::sync::Arc;

use chrono::Utc;

use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use crate::domain::repositories::{AutomationRepository, AutomationRunRepository};

use super::{MemoryAutomationRepository, MemoryAutomationRunRepository};

fn automation(id: &str, project_id: &str, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string(project_id.to_string()),
        name: format!("Automation {id}"),
        status,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Implement the plan".to_string(),
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
        first_run_prompt: Some("Run 1".to_string()),
        setup_analysis_summary: None,
        created_at: now,
        updated_at: now,
    }
}

fn run(
    id: &str,
    automation_id: &str,
    index: i64,
    status: AutomationRunStatus,
    judge_state: AutomationJudgeState,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: AutomationId::from_string(automation_id),
        run_index: index,
        status,
        judge_state,
        judge_lease_expires_at: None,
        conversation_id: None,
        run_prompt: format!("Run {index} prompt"),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: String::new(),
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

#[tokio::test]
async fn memory_automation_repo_cas_and_project_listing() {
    let repo = MemoryAutomationRepository::new();
    let first = automation("automation-1", "project-1", AutomationStatus::Draft);
    let other = automation("automation-2", "project-2", AutomationStatus::Active);

    repo.create(first.clone()).await.unwrap();
    repo.create(other.clone()).await.unwrap();

    assert_eq!(
        repo.list_by_project(&first.project_id).await.unwrap(),
        vec![first.clone()]
    );
    assert_eq!(
        repo.list(None).await.unwrap(),
        vec![other.clone(), first.clone()]
    );
    let updated = repo
        .update_settings(
            &first.id,
            crate::domain::repositories::AutomationSettingsPatch {
                name: Some("Renamed".to_string()),
                max_runs: Some(9),
                max_consecutive_failures: Some(4),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.max_runs, 9);
    assert_eq!(updated.max_consecutive_failures, 4);
    assert_eq!(updated.status, AutomationStatus::Draft);
    assert!(repo
        .compare_and_swap_status(
            &first.id,
            AutomationStatus::Draft,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
        .unwrap());
    assert!(!repo
        .compare_and_swap_status(
            &first.id,
            AutomationStatus::Draft,
            AutomationStatus::Stopped,
            None,
            None,
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_id(&first.id).await.unwrap().unwrap().status,
        AutomationStatus::Active
    );
    assert!(!repo.delete_terminal(&first.id).await.unwrap());
    assert!(repo
        .compare_and_swap_status(
            &first.id,
            AutomationStatus::Active,
            AutomationStatus::Stopped,
            None,
            None,
        )
        .await
        .unwrap());
    assert!(repo.delete_terminal(&first.id).await.unwrap());
    assert!(repo.get_by_id(&first.id).await.unwrap().is_none());
}

#[tokio::test]
async fn memory_run_repo_enforces_open_run_single_flight() {
    let repo = Arc::new(MemoryAutomationRunRepository::new());
    repo.create_run(run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::None,
    ))
    .await
    .unwrap();

    let duplicate = repo
        .create_run(run(
            "run-2",
            "automation-1",
            2,
            AutomationRunStatus::Pending,
            AutomationJudgeState::None,
        ))
        .await;
    assert!(duplicate.is_err());

    assert!(repo
        .compare_and_swap_judge_state(
            &AutomationRunId::from_string("run-1"),
            AutomationJudgeState::None,
            AutomationJudgeState::Done,
            None,
            None,
        )
        .await
        .unwrap());
    repo.create_run(run(
        "run-2",
        "automation-1",
        2,
        AutomationRunStatus::Pending,
        AutomationJudgeState::None,
    ))
    .await
    .unwrap();

    assert_eq!(
        repo.delete_for_automation(&AutomationId::from_string("automation-1"))
            .await
            .unwrap(),
        2
    );
    assert!(repo
        .list_for_automation(&AutomationId::from_string("automation-1"))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn memory_run_repo_clears_stale_judge_verdict_when_retry_starts() {
    let repo = MemoryAutomationRunRepository::new();
    let mut failed = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    failed.judge_verdict_json = Some(r#"{"result":"old"}"#.to_string());
    failed.error_detail = Some("previous judge attempt failed".to_string());
    repo.create_run(failed.clone()).await.unwrap();

    assert!(repo
        .compare_and_swap_judge_state(
            &failed.id,
            AutomationJudgeState::Failed,
            AutomationJudgeState::InProgress,
            None,
            None,
        )
        .await
        .unwrap());

    let updated = repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(updated.judge_state, AutomationJudgeState::InProgress);
    assert_eq!(updated.judge_verdict_json, None);
    assert_eq!(updated.error_detail, None);
}
