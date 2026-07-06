use std::sync::Arc;

use chrono::Utc;

use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use crate::domain::repositories::{
    AutomationConfigPatch, AutomationRepository, AutomationRunRepository,
};

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

fn successor_run(
    id: &str,
    automation_id: &str,
    index: i64,
    previous_run_id: &AutomationRunId,
) -> AutomationRun {
    let mut run = run(
        id,
        automation_id,
        index,
        AutomationRunStatus::Pending,
        AutomationJudgeState::None,
    );
    run.prompt_author = AutomationPromptAuthor::SkipJudgeTemplate;
    run.run_prompt = "Continue the goal; previous run merged PR #593.".to_string();
    run.base_from_run_id = Some(previous_run_id.clone());
    run
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
async fn memory_automation_repo_update_config_writes_only_provided_fields() {
    let repo = MemoryAutomationRepository::new();
    let mut automation = automation("automation-1", "project-1", AutomationStatus::Draft);
    automation.goal_prompt = String::new();
    automation.first_run_prompt = None;
    automation.base_ref = String::new();
    repo.create(automation.clone()).await.unwrap();

    let updated = repo
        .update_config(
            &automation.id,
            AutomationConfigPatch {
                goal_prompt: Some("Ship the migration".to_string()),
                first_run_prompt: Some("Implement item 1 in a scoped PR.".to_string()),
                base_ref_kind: Some("local_branch".to_string()),
                base_ref: Some("main".to_string()),
                model_id: Some("gpt-5.4".to_string()),
                goal_items_json: Some(
                    r#"[{"id":"phase-1","title":"Create context model","status":"pending"}]"#
                        .to_string(),
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .expect("config should update");

    assert_eq!(updated.goal_prompt, "Ship the migration");
    assert_eq!(
        updated.first_run_prompt.as_deref(),
        Some("Implement item 1 in a scoped PR.")
    );
    assert_eq!(updated.base_ref_kind, "local_branch");
    assert_eq!(updated.base_ref, "main");
    assert_eq!(updated.model_id, "gpt-5.4");
    assert_eq!(
        updated.goal_items_json.as_deref(),
        Some(r#"[{"id":"phase-1","title":"Create context model","status":"pending"}]"#),
    );
    // Fields left None keep their pre-existing values (parity with SQLite).
    assert_eq!(updated.provider_harness, "claude");
    assert_eq!(updated.chain_mode, "merged_base");
    assert_eq!(updated.completion_signal, "pr_merged");
    assert_eq!(updated.status, AutomationStatus::Draft);

    let stored = repo.get_by_id(&automation.id).await.unwrap().unwrap();
    assert_eq!(stored, updated);

    assert!(repo
        .update_config(
            &AutomationId::from_string("missing-automation"),
            AutomationConfigPatch {
                goal_prompt: Some("nope".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .is_none());
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
async fn memory_run_repo_skip_judge_and_successor_are_atomic() {
    let repo = Arc::new(MemoryAutomationRunRepository::new());
    let previous = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    repo.create_run(previous.clone()).await.unwrap();

    let created = repo
        .skip_judge_and_create_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            successor_run("run-2", "automation-1", 2, &previous.id),
        )
        .await
        .unwrap()
        .expect("successor should be created");

    assert_eq!(created.id, AutomationRunId::from_string("run-2"));
    let runs = repo
        .list_for_automation(&AutomationId::from_string("automation-1"))
        .await
        .unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].judge_state, AutomationJudgeState::Skipped);
    assert_eq!(
        runs[1].prompt_author,
        AutomationPromptAuthor::SkipJudgeTemplate
    );
    assert_eq!(runs[1].base_from_run_id, Some(previous.id.clone()));

    let stale = repo
        .skip_judge_and_create_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            successor_run("run-3", "automation-1", 3, &previous.id),
        )
        .await
        .unwrap();
    assert!(stale.is_none());
    assert_eq!(
        repo.list_for_automation(&AutomationId::from_string("automation-1"))
            .await
            .unwrap()
            .len(),
        2
    );
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

    let lease_expires_at = Utc::now() + chrono::Duration::minutes(3);
    assert!(repo
        .compare_and_swap_judge_state(
            &failed.id,
            AutomationJudgeState::Failed,
            AutomationJudgeState::InProgress,
            None,
            None,
            Some(lease_expires_at),
            None,
        )
        .await
        .unwrap());

    let updated = repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(updated.judge_state, AutomationJudgeState::InProgress);
    assert_eq!(updated.judge_verdict_json, None);
    assert_eq!(updated.error_detail, None);
    assert_eq!(updated.judge_lease_expires_at, Some(lease_expires_at));
}
