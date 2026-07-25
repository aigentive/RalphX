use std::sync::Arc;

use chrono::{TimeZone, Utc};
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;

use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversationId, ProjectId,
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
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
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
        run_prompt: format!("Run {index} prompt"),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: String::new(),
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

#[tokio::test]
async fn memory_run_repo_atomically_merges_status_and_metadata() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    let merged_at = Utc.with_ymd_and_hms(2026, 7, 5, 12, 0, 0).unwrap();
    let mut published = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::Published,
        AutomationJudgeState::None,
    );
    published.signal_check_failures = 2;
    published.error_code = Some("warning".to_string());
    published.error_detail = Some("non-terminal warning".to_string());
    repo.create_run(published.clone()).await.unwrap();

    assert!(repo
        .compare_and_swap_status_with_merge_metadata(
            &published.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            Some("merge-sha".to_string()),
            Some(merged_at),
        )
        .await
        .unwrap());

    let stored = repo.get_by_id(&published.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationRunStatus::Merged);
    assert_eq!(stored.merge_commit_sha.as_deref(), Some("merge-sha"));
    assert_eq!(stored.pr_merged_at, Some(merged_at));
    assert_eq!(stored.signal_check_failures, 0);
    assert!(stored.error_code.is_none());
    assert!(stored.error_detail.is_none());
    assert!(stored.finished_at.is_some());

    assert!(!repo
        .compare_and_swap_status_with_merge_metadata(
            &published.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            Some("late-sha".to_string()),
            None,
        )
        .await
        .unwrap());
    let unchanged = repo.get_by_id(&published.id).await.unwrap().unwrap();
    assert_eq!(unchanged.merge_commit_sha.as_deref(), Some("merge-sha"));
    assert_eq!(unchanged.pr_merged_at, Some(merged_at));
}

#[tokio::test]
async fn memory_automation_repo_round_trips_plan_gate_config_fields() {
    let repo = MemoryAutomationRepository::new();
    let mut automation = automation("automation-1", "project-1", AutomationStatus::Draft);
    automation.plan_approval_mode = AutomationPlanApprovalMode::Automatic;
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation.plan_deep_verification = true;

    repo.create(automation.clone()).await.unwrap();

    let stored = repo.get_by_id(&automation.id).await.unwrap().unwrap();
    assert_eq!(
        stored.plan_approval_mode,
        AutomationPlanApprovalMode::Automatic
    );
    assert_eq!(stored.pr_merge_mode, AutomationPrMergeMode::Automatic);
    assert!(stored.plan_deep_verification);
    assert_eq!(stored, automation);
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
                plan_approval_mode: Some(AutomationPlanApprovalMode::Automatic),
                pr_merge_mode: Some(AutomationPrMergeMode::Automatic),
                plan_deep_verification: Some(true),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.max_runs, 9);
    assert_eq!(updated.max_consecutive_failures, 4);
    assert_eq!(
        updated.plan_approval_mode,
        AutomationPlanApprovalMode::Automatic
    );
    assert_eq!(updated.pr_merge_mode, AutomationPrMergeMode::Automatic);
    assert!(updated.plan_deep_verification);
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
                spec_artifact_id: Some("artifact-spec-1".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .expect("config should update");

    assert_eq!(updated.goal_prompt, "Ship the migration");
    assert_eq!(updated.spec_artifact_id.as_deref(), Some("artifact-spec-1"));
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
async fn memory_automation_repo_goal_items_cas_matches_only_expected_json() {
    let repo = MemoryAutomationRepository::new();
    let mut automation = automation("automation-1", "project-1", AutomationStatus::Draft);
    automation.goal_items_json = Some(r#"[{"id":"item-1","status":"pending"}]"#.to_string());
    repo.create(automation.clone()).await.unwrap();

    let updated = repo
        .update_goal_items_json_if_unchanged(
            &automation.id,
            automation.goal_items_json.clone(),
            Some(r#"[{"id":"item-1","status":"in_progress"}]"#.to_string()),
        )
        .await
        .unwrap()
        .expect("expected JSON should match");
    assert_eq!(
        updated.goal_items_json.as_deref(),
        Some(r#"[{"id":"item-1","status":"in_progress"}]"#)
    );

    assert!(repo
        .update_goal_items_json_if_unchanged(
            &automation.id,
            automation.goal_items_json.clone(),
            Some(r#"[{"id":"item-1","status":"done"}]"#.to_string()),
        )
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.get_by_id(&automation.id)
            .await
            .unwrap()
            .unwrap()
            .goal_items_json
            .as_deref(),
        Some(r#"[{"id":"item-1","status":"in_progress"}]"#)
    );
}

#[tokio::test]
async fn memory_run_repo_enforces_open_run_single_flight() {
    let repo = Arc::new(MemoryAutomationRunRepository::new(
        MemoryAutomationRepository::new_shared_state(),
    ));
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
            AutomationJudgeTransitionGuard::Dispatch,
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
    let automation_repo = MemoryAutomationRepository::new();
    automation_repo
        .create(automation(
            "automation-1",
            "project-1",
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let previous = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::Completed,
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
async fn memory_run_repo_create_judge_successor_requires_active_latest_done_signal_terminal() {
    let automation_repo = MemoryAutomationRepository::new();
    automation_repo
        .create(automation(
            "automation-1",
            "project-1",
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let repo = MemoryAutomationRunRepository::new(automation_repo.shared_state());
    let previous = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    repo.create_run(previous.clone()).await.unwrap();

    assert!(repo
        .create_judge_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            successor_run("run-2", "automation-1", 2, &previous.id),
        )
        .await
        .unwrap()
        .is_some());

    let paused_automation_repo = MemoryAutomationRepository::new();
    paused_automation_repo
        .create(automation(
            "automation-paused",
            "project-1",
            AutomationStatus::Paused,
        ))
        .await
        .unwrap();
    let paused_repo = MemoryAutomationRunRepository::new(paused_automation_repo.shared_state());
    let paused_previous = run(
        "run-paused-1",
        "automation-paused",
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    paused_repo
        .create_run(paused_previous.clone())
        .await
        .unwrap();
    assert!(paused_repo
        .create_judge_successor_run(
            &AutomationId::from_string("automation-paused"),
            &paused_previous.id,
            successor_run("run-paused-2", "automation-paused", 2, &paused_previous.id),
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn memory_run_repo_clears_stale_judge_verdict_when_retry_starts() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
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
            AutomationJudgeTransitionGuard::Dispatch,
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

    let stale_lease = lease_expires_at + chrono::Duration::minutes(1);
    assert!(!repo
        .compare_and_swap_judge_state(
            &failed.id,
            AutomationJudgeState::InProgress,
            AutomationJudgeState::Done,
            AutomationJudgeTransitionGuard::Settle(stale_lease),
            Some(r#"{"decision":"stop"}"#.to_string()),
            Some("haiku".to_string()),
            None,
            None,
        )
        .await
        .unwrap());
    let still_in_progress = repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(
        still_in_progress.judge_state,
        AutomationJudgeState::InProgress
    );
    assert_eq!(still_in_progress.judge_verdict_json, None);
    assert_eq!(still_in_progress.judge_model_id, None);
    assert_eq!(
        still_in_progress.judge_lease_expires_at,
        Some(lease_expires_at)
    );

    assert!(repo
        .compare_and_swap_judge_state(
            &failed.id,
            AutomationJudgeState::InProgress,
            AutomationJudgeState::Done,
            AutomationJudgeTransitionGuard::Settle(lease_expires_at),
            Some(r#"{"decision":"stop"}"#.to_string()),
            Some("haiku".to_string()),
            None,
            None,
        )
        .await
        .unwrap());
    let completed = repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(completed.judge_state, AutomationJudgeState::Done);
    assert_eq!(
        completed.judge_verdict_json.as_deref(),
        Some(r#"{"decision":"stop"}"#)
    );
    assert_eq!(completed.judge_model_id.as_deref(), Some("haiku"));
    assert_eq!(completed.judge_lease_expires_at, None);
}

#[tokio::test]
async fn memory_run_repo_round_trips_and_updates_plan_gate_fields() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let agent_phase_started_at = Utc::now();
    let mut run = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::InProgress;
    run.plan_judge_lease_expires_at = Some(lease_expires_at);
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    run.plan_revision_round = 2;
    run.plan_reminder_count = 1;
    run.plan_pending_instructions = Some("Tighten the rollout section.".to_string());
    run.plan_last_parked_artifact_id = Some("artifact-plan-1".to_string());
    run.agent_phase_started_at = Some(agent_phase_started_at);
    repo.create_run(run.clone()).await.unwrap();

    let stored = repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(stored, run);

    assert!(repo
        .compare_and_swap_plan_judge_state(
            &run.id,
            AutomationPlanJudgeState::InProgress,
            AutomationPlanJudgeState::Done,
            Some(r#"{"decision":"approve"}"#.to_string()),
            None,
        )
        .await
        .unwrap());
    let updated = repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(updated.plan_judge_state, AutomationPlanJudgeState::Done);
    assert_eq!(
        updated.plan_judge_verdict_json.as_deref(),
        Some(r#"{"decision":"approve"}"#)
    );
    assert_eq!(updated.plan_judge_lease_expires_at, None);

    assert!(repo
        .set_plan_pending_instructions(&run.id, None)
        .await
        .unwrap()
        .unwrap()
        .plan_pending_instructions
        .is_none());
    assert_eq!(
        repo.set_plan_revision_round(&run.id, 3)
            .await
            .unwrap()
            .unwrap()
            .plan_revision_round,
        3
    );
    assert_eq!(
        repo.set_plan_last_parked_artifact_id(&run.id, Some("artifact-plan-2".to_string()))
            .await
            .unwrap()
            .unwrap()
            .plan_last_parked_artifact_id
            .as_deref(),
        Some("artifact-plan-2")
    );
    assert_eq!(
        repo.set_plan_reminder_count(&run.id, 2)
            .await
            .unwrap()
            .unwrap()
            .plan_reminder_count,
        2
    );
    let new_phase_started_at = Utc::now() + chrono::Duration::minutes(1);
    assert_eq!(
        repo.set_agent_phase_started_at(&run.id, Some(new_phase_started_at))
            .await
            .unwrap()
            .unwrap()
            .agent_phase_started_at,
        Some(new_phase_started_at)
    );
}

#[tokio::test]
async fn memory_run_repo_status_cas_with_agent_phase_started_at_uses_observed_phase() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    let run = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    repo.create_run(run.clone()).await.unwrap();
    let observed_phase_started_at = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();

    assert!(repo
        .compare_and_swap_status_with_agent_phase_started_at(
            &run.id,
            AutomationRunStatus::AwaitingPlanApproval,
            AutomationRunStatus::Running,
            observed_phase_started_at,
            None,
            None,
        )
        .await
        .unwrap());

    let updated = repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(updated.status, AutomationRunStatus::Running);
    assert_eq!(
        updated.agent_phase_started_at,
        Some(observed_phase_started_at)
    );
}

#[tokio::test]
async fn memory_plan_judge_cas_rejects_wrong_from_without_mutating_fields() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let mut run = run(
        "run-plan-cas-stale",
        "automation-1",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::InProgress;
    run.plan_judge_lease_expires_at = Some(lease_expires_at);
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    repo.create_run(run.clone()).await.unwrap();

    assert!(!repo
        .compare_and_swap_plan_judge_state(
            &run.id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::Done,
            Some(r#"{"decision":"approve"}"#.to_string()),
            None,
        )
        .await
        .unwrap());

    let unchanged = repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(
        unchanged.plan_judge_state,
        AutomationPlanJudgeState::InProgress
    );
    assert_eq!(
        unchanged.plan_judge_lease_expires_at,
        Some(lease_expires_at)
    );
    assert_eq!(
        unchanged.plan_judge_verdict_json.as_deref(),
        Some(r#"{"decision":"revise"}"#)
    );
}

#[tokio::test]
async fn memory_clear_plan_judge_verdict_preserves_plan_judge_state() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let mut run = run(
        "run-plan-clear-verdict",
        "automation-1",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::Done;
    run.plan_judge_lease_expires_at = Some(lease_expires_at);
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    repo.create_run(run.clone()).await.unwrap();

    assert!(repo.clear_plan_judge_verdict(&run.id).await.unwrap());
    let cleared = repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(cleared.plan_judge_state, AutomationPlanJudgeState::Done);
    assert_eq!(cleared.plan_judge_lease_expires_at, Some(lease_expires_at));
    assert_eq!(cleared.plan_judge_verdict_json, None);

    let missing = AutomationRunId::from_string("missing-run");
    assert!(!repo.clear_plan_judge_verdict(&missing).await.unwrap());
}

#[tokio::test]
async fn memory_plan_judge_dispatch_sets_lease_and_preserves_stored_verdict() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let mut run = run(
        "run-plan-cas-dispatch",
        "automation-1",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    repo.create_run(run.clone()).await.unwrap();

    assert!(repo
        .compare_and_swap_plan_judge_state(
            &run.id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::InProgress,
            None,
            Some(lease_expires_at),
        )
        .await
        .unwrap());

    let dispatched = repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(
        dispatched.plan_judge_state,
        AutomationPlanJudgeState::InProgress
    );
    assert_eq!(
        dispatched.plan_judge_lease_expires_at,
        Some(lease_expires_at)
    );
    assert_eq!(
        dispatched.plan_judge_verdict_json.as_deref(),
        Some(r#"{"decision":"revise"}"#)
    );
}

#[tokio::test]
async fn memory_automation_repo_child_row_deletes_are_noop_ok() {
    // The in-memory repo does not model attachment / context-ref rows, so the
    // deletes mirror the SQLite contract by returning Ok(0) instead of erroring.
    let repo = MemoryAutomationRepository::new();
    let automation_id = AutomationId::from_string("automation-1");
    assert_eq!(
        repo.delete_attachments_for_automation(&automation_id)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        repo.delete_context_refs_for_automation(&automation_id)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn memory_find_run_by_conversation_id_returns_latest_linked_run() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    // Valid distinct UUIDs — from_string collapses non-UUID text to Uuid::nil().
    let conversation = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");

    let mut first = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    first.conversation_id = Some(conversation.clone());
    let mut second = run(
        "run-2",
        "automation-1",
        2,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    second.conversation_id = Some(conversation.clone());
    // Different automation so the single-open-run constraint is not tripped.
    let mut unrelated = run(
        "run-3",
        "automation-2",
        3,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    unrelated.conversation_id = Some(ChatConversationId::from_string(
        "22222222-2222-2222-2222-222222222222",
    ));

    repo.create_run(first).await.unwrap();
    repo.create_run(second.clone()).await.unwrap();
    repo.create_run(unrelated).await.unwrap();

    let found = repo
        .find_run_by_conversation_id(&conversation)
        .await
        .unwrap()
        .expect("linked run should be found");
    assert_eq!(found.id, second.id, "highest run_index wins");

    assert!(repo
        .find_run_by_conversation_id(&ChatConversationId::from_string(
            "99999999-9999-9999-9999-999999999999"
        ))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn memory_run_repo_round_trips_goal_item_id() {
    let repo = MemoryAutomationRunRepository::new(MemoryAutomationRepository::new_shared_state());
    let mut mapped = run(
        "run-1",
        "automation-1",
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    mapped.goal_item_id = Some("item-b1".to_string());
    repo.create_run(mapped.clone()).await.unwrap();

    let stored = repo.get_by_id(&mapped.id).await.unwrap().unwrap();
    assert_eq!(stored.goal_item_id.as_deref(), Some("item-b1"));
    assert_eq!(stored, mapped);
}
