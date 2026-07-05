use chrono::Utc;

use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use crate::domain::repositories::{AutomationRepository, AutomationRunRepository};
use crate::testing::SqliteTestDb;

use super::{SqliteAutomationRepository, SqliteAutomationRunRepository};

fn setup_repos() -> (
    SqliteTestDb,
    ProjectId,
    SqliteAutomationRepository,
    SqliteAutomationRunRepository,
) {
    let db = SqliteTestDb::new("sqlite_automation_repo_tests");
    let project = db.seed_project("Project 1");
    let project_id = project.id;
    let automation_repo = SqliteAutomationRepository::from_shared(db.shared_conn());
    let run_repo = SqliteAutomationRunRepository::from_shared(db.shared_conn());
    (db, project_id, automation_repo, run_repo)
}

fn automation(id: &str, project_id: ProjectId, status: AutomationStatus) -> Automation {
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
    index: i64,
    status: AutomationRunStatus,
    judge_state: AutomationJudgeState,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: AutomationId::from_string("automation-1"),
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
async fn sqlite_automation_repo_cas_and_project_listing() {
    let (_db, project_id, repo, _run_repo) = setup_repos();
    let automation = automation("automation-1", project_id, AutomationStatus::Draft);

    repo.create(automation.clone()).await.unwrap();

    assert_eq!(
        repo.list_by_project(&automation.project_id).await.unwrap(),
        vec![automation.clone()]
    );
    assert_eq!(repo.list(None).await.unwrap(), vec![automation.clone()]);
    let updated = repo
        .update_settings(
            &automation.id,
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
            &automation.id,
            AutomationStatus::Draft,
            AutomationStatus::Paused,
            Some("user".to_string()),
            Some("manual pause".to_string()),
        )
        .await
        .unwrap());
    assert!(!repo
        .compare_and_swap_status(
            &automation.id,
            AutomationStatus::Draft,
            AutomationStatus::Stopped,
            None,
            None,
        )
        .await
        .unwrap());
    let updated = repo.get_by_id(&automation.id).await.unwrap().unwrap();
    assert_eq!(updated.status, AutomationStatus::Paused);
    assert_eq!(updated.paused_reason_code.as_deref(), Some("user"));
    assert!(!repo.delete_terminal(&automation.id).await.unwrap());
    assert!(repo
        .compare_and_swap_status(
            &automation.id,
            AutomationStatus::Paused,
            AutomationStatus::Stopped,
            None,
            None,
        )
        .await
        .unwrap());
    assert!(repo.delete_terminal(&automation.id).await.unwrap());
    assert!(repo.get_by_id(&automation.id).await.unwrap().is_none());
}

#[tokio::test]
async fn sqlite_run_repo_latest_and_single_open_index() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();

    run_repo
        .create_run(run(
            "run-1",
            1,
            AutomationRunStatus::AgentFailed,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();
    let duplicate_open = run_repo
        .create_run(run(
            "run-2",
            2,
            AutomationRunStatus::Pending,
            AutomationJudgeState::None,
        ))
        .await;
    assert!(matches!(
        duplicate_open,
        Err(crate::error::AppError::Conflict(_))
    ));

    assert!(run_repo
        .compare_and_swap_judge_state(
            &AutomationRunId::from_string("run-1"),
            AutomationJudgeState::None,
            AutomationJudgeState::Done,
            None,
            None,
        )
        .await
        .unwrap());
    run_repo
        .create_run(run(
            "run-2",
            2,
            AutomationRunStatus::Pending,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();

    assert_eq!(
        run_repo
            .latest_for_automation(&AutomationId::from_string("automation-1"))
            .await
            .unwrap()
            .unwrap()
            .id,
        AutomationRunId::from_string("run-2")
    );
    assert_eq!(
        run_repo
            .delete_for_automation(&AutomationId::from_string("automation-1"))
            .await
            .unwrap(),
        2
    );
    assert!(run_repo
        .list_for_automation(&AutomationId::from_string("automation-1"))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn sqlite_run_repo_clears_stale_judge_verdict_when_retry_starts() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let mut failed = run(
        "run-1",
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    failed.judge_verdict_json = Some(r#"{"result":"old"}"#.to_string());
    failed.error_detail = Some("previous judge attempt failed".to_string());
    run_repo.create_run(failed.clone()).await.unwrap();

    assert!(run_repo
        .compare_and_swap_judge_state(
            &failed.id,
            AutomationJudgeState::Failed,
            AutomationJudgeState::InProgress,
            None,
            None,
        )
        .await
        .unwrap());

    let updated = run_repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(updated.judge_state, AutomationJudgeState::InProgress);
    assert_eq!(updated.judge_verdict_json, None);
    assert_eq!(updated.error_detail, None);
}
