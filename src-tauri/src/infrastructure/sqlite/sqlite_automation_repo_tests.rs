use chrono::{TimeZone, Utc};
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;

use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversation, ChatConversationId,
    ProjectId,
};
use crate::domain::repositories::{
    AutomationConfigPatch, AutomationRepository, AutomationRunPublicationMetadata,
    AutomationRunRepository, AutomationSettingsPatch,
};
use crate::error::AppError;
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
async fn sqlite_run_repo_atomically_merges_status_and_metadata() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    let merged_at = Utc.with_ymd_and_hms(2026, 7, 5, 12, 0, 0).unwrap();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let mut published = run(
        "run-1",
        1,
        AutomationRunStatus::Published,
        AutomationJudgeState::None,
    );
    published.signal_check_failures = 2;
    published.error_code = Some("warning".to_string());
    published.error_detail = Some("non-terminal warning".to_string());
    run_repo.create_run(published.clone()).await.unwrap();

    assert!(run_repo
        .compare_and_swap_status_with_merge_metadata(
            &published.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            Some("merge-sha".to_string()),
            Some(merged_at),
        )
        .await
        .unwrap());

    let stored = run_repo.get_by_id(&published.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationRunStatus::Merged);
    assert_eq!(stored.merge_commit_sha.as_deref(), Some("merge-sha"));
    assert_eq!(stored.pr_merged_at, Some(merged_at));
    assert_eq!(stored.signal_check_failures, 0);
    assert!(stored.error_code.is_none());
    assert!(stored.error_detail.is_none());
    assert!(stored.finished_at.is_some());

    assert!(!run_repo
        .compare_and_swap_status_with_merge_metadata(
            &published.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            Some("late-sha".to_string()),
            None,
        )
        .await
        .unwrap());
    let unchanged = run_repo.get_by_id(&published.id).await.unwrap().unwrap();
    assert_eq!(unchanged.merge_commit_sha.as_deref(), Some("merge-sha"));
    assert_eq!(unchanged.pr_merged_at, Some(merged_at));
}

#[tokio::test]
async fn sqlite_automation_repo_round_trips_plan_gate_config_fields() {
    let (_db, project_id, repo, _run_repo) = setup_repos();
    let mut automation = automation("automation-1", project_id, AutomationStatus::Draft);
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

fn successor_run(id: &str, index: i64, previous_run_id: &AutomationRunId) -> AutomationRun {
    let mut run = run(
        id,
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
async fn sqlite_automation_repo_updates_goal_items_and_maps_optional_fields() {
    let (db, project_id, repo, _run_repo) = setup_repos();
    let setup_conversation_id =
        ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let mut setup_conversation = ChatConversation::new_project(project_id.clone());
    setup_conversation.id = setup_conversation_id;
    db.insert_conversation(setup_conversation);

    let mut automation = automation("automation-1", project_id, AutomationStatus::Draft);
    automation.paused_reason_code = Some("user".to_string());
    automation.paused_reason_detail = Some("paused for edit".to_string());
    automation.setup_conversation_id = Some(setup_conversation_id);
    automation.logical_effort = Some("medium".to_string());
    automation.base_display_name = Some("Project default (main)".to_string());
    automation.base_source_pull_request_json = Some(r#"{"number":593}"#.to_string());
    automation.goal_items_json = Some(r#"[{"id":"item-1","status":"pending"}]"#.to_string());
    automation.setup_analysis_summary = Some("setup analysis".to_string());

    repo.create(automation.clone()).await.unwrap();

    let stored = repo.get_by_id(&automation.id).await.unwrap().unwrap();
    assert_eq!(stored, automation);

    let updated = repo
        .update_goal_items_json(
            &automation.id,
            Some(r#"[{"id":"item-1","status":"done"}]"#.to_string()),
        )
        .await
        .unwrap()
        .expect("goal items should update");
    assert_eq!(
        updated.goal_items_json.as_deref(),
        Some(r#"[{"id":"item-1","status":"done"}]"#)
    );

    let cleared = repo
        .update_goal_items_json(&automation.id, None)
        .await
        .unwrap()
        .expect("goal items should clear");
    assert_eq!(cleared.goal_items_json, None);

    let cas_from_null = repo
        .update_goal_items_json_if_unchanged(
            &automation.id,
            None,
            Some(r#"[{"id":"item-1","status":"pending"}]"#.to_string()),
        )
        .await
        .unwrap()
        .expect("null expected value should match");
    assert_eq!(
        cas_from_null.goal_items_json.as_deref(),
        Some(r#"[{"id":"item-1","status":"pending"}]"#)
    );
    assert!(repo
        .update_goal_items_json_if_unchanged(
            &automation.id,
            Some(r#"[{"id":"item-1","status":"done"}]"#.to_string()),
            Some(r#"[{"id":"item-1","status":"in_progress"}]"#.to_string()),
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
        Some(r#"[{"id":"item-1","status":"pending"}]"#)
    );

    assert!(repo
        .update_goal_items_json(
            &AutomationId::from_string("missing-automation"),
            Some("[]".to_string()),
        )
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .update_settings(
            &AutomationId::from_string("missing-automation"),
            AutomationSettingsPatch {
                name: Some("No row".to_string()),
                max_runs: None,
                max_consecutive_failures: None,
                plan_approval_mode: None,
                pr_merge_mode: None,
                plan_deep_verification: None,
            },
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_automation_repo_update_config_writes_only_provided_fields() {
    let (_db, project_id, repo, _run_repo) = setup_repos();
    let mut automation = automation("automation-1", project_id, AutomationStatus::Draft);
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
    // Fields left None keep their pre-existing values.
    assert_eq!(updated.provider_harness, "claude");
    assert_eq!(updated.chain_mode, "merged_base");
    assert_eq!(updated.completion_signal, "pr_merged");
    assert_eq!(updated.status, AutomationStatus::Draft);

    // Persisted round-trip matches the returned row.
    let stored = repo.get_by_id(&automation.id).await.unwrap().unwrap();
    assert_eq!(stored, updated);

    // Unknown id yields None without writing.
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
            AutomationJudgeTransitionGuard::Dispatch,
            None,
            None,
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
async fn sqlite_run_repo_updates_attempt_metadata_and_signal_failures() {
    let (db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id.clone(),
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let run = run(
        "run-1",
        1,
        AutomationRunStatus::Provisioning,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id;
    conversation.automation_id = Some(AutomationId::from_string("automation-1"));
    conversation.automation_run_id = Some(run.id.clone());
    db.insert_conversation(conversation);

    let started = run_repo
        .update_start_metadata(
            &run.id,
            &conversation_id,
            Some("ralphx/automation-run-1".to_string()),
        )
        .await
        .unwrap()
        .expect("provisioning run should accept start metadata");
    assert_eq!(started.conversation_id, Some(conversation_id));
    assert_eq!(
        started.branch_name.as_deref(),
        Some("ralphx/automation-run-1")
    );
    assert!(started.started_at.is_some());

    assert!(run_repo
        .compare_and_swap_status(
            &run.id,
            AutomationRunStatus::Provisioning,
            AutomationRunStatus::Running,
            None,
            None,
        )
        .await
        .unwrap());
    assert_eq!(
        run_repo
            .get_by_id(&run.id)
            .await
            .unwrap()
            .unwrap()
            .finished_at,
        None
    );

    let published_metadata = AutomationRunPublicationMetadata {
        pr_number: Some(593),
        pr_url: Some("https://github.com/aigentive/ralphx.app/pull/593".to_string()),
        pr_title: Some("Automation run 1".to_string()),
        pr_head_ref_name: Some("ralphx/automation-run-1".to_string()),
        pr_base_ref_name: Some("main".to_string()),
    };
    let published = run_repo
        .update_publication_metadata(&run.id, published_metadata)
        .await
        .unwrap()
        .expect("running run should accept publication metadata");
    assert_eq!(published.pr_number, Some(593));
    assert_eq!(published.pr_base_ref_name.as_deref(), Some("main"));

    assert!(run_repo
        .compare_and_swap_status(
            &run.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::Published,
            None,
            None,
        )
        .await
        .unwrap());

    let first_failure = run_repo
        .increment_signal_check_failures(&run.id)
        .await
        .unwrap()
        .expect("published run should increment signal failures");
    assert_eq!(first_failure.signal_check_failures, 1);
    let second_failure = run_repo
        .increment_signal_check_failures(&run.id)
        .await
        .unwrap()
        .expect("published run should increment signal failures again");
    assert_eq!(second_failure.signal_check_failures, 2);
    let reset = run_repo
        .reset_signal_check_failures(&run.id)
        .await
        .unwrap()
        .expect("published run should reset signal failures");
    assert_eq!(reset.signal_check_failures, 0);

    let merged_at = Utc::now();
    let merge_metadata = run_repo
        .update_merge_metadata(&run.id, Some("merge-sha".to_string()), Some(merged_at))
        .await
        .unwrap()
        .expect("published run should accept merge metadata");
    assert_eq!(merge_metadata.pr_merged_at, Some(merged_at));
    assert_eq!(
        merge_metadata.merge_commit_sha.as_deref(),
        Some("merge-sha")
    );
    assert_eq!(merge_metadata.signal_check_failures, 0);

    assert!(run_repo
        .compare_and_swap_status(
            &run.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Merged,
            None,
            None,
        )
        .await
        .unwrap());
    let terminal = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(terminal.status, AutomationRunStatus::Merged);
    assert!(terminal.finished_at.is_some());

    assert!(run_repo
        .update_start_metadata(
            &run.id,
            &ChatConversationId::from_string("late-conversation"),
            Some("late-branch".to_string()),
        )
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .update_publication_metadata(&run.id, AutomationRunPublicationMetadata::default())
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .update_merge_metadata(&run.id, Some("late-sha".to_string()), None)
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .increment_signal_check_failures(&run.id)
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .reset_signal_check_failures(&run.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_run_repo_skip_judge_and_successor_are_atomic() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let previous = run(
        "run-1",
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::None,
    );
    run_repo.create_run(previous.clone()).await.unwrap();

    let created = run_repo
        .skip_judge_and_create_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            successor_run("run-2", 2, &previous.id),
        )
        .await
        .unwrap()
        .expect("successor should be created");

    assert_eq!(created.id, AutomationRunId::from_string("run-2"));
    let runs = run_repo
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

    let stale = run_repo
        .skip_judge_and_create_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            successor_run("run-3", 3, &previous.id),
        )
        .await
        .unwrap();
    assert!(stale.is_none());
    assert_eq!(
        run_repo
            .list_for_automation(&AutomationId::from_string("automation-1"))
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn sqlite_run_repo_create_judge_successor_requires_active_latest_done_signal_terminal() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id.clone(),
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let previous = run(
        "run-1",
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();

    assert!(run_repo
        .create_judge_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            successor_run("run-2", 2, &previous.id),
        )
        .await
        .unwrap()
        .is_some());

    automation_repo
        .create(automation(
            "automation-paused",
            project_id,
            AutomationStatus::Paused,
        ))
        .await
        .unwrap();
    let mut paused_previous = run(
        "run-paused-1",
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    paused_previous.automation_id = AutomationId::from_string("automation-paused");
    run_repo.create_run(paused_previous.clone()).await.unwrap();
    let mut paused_successor = successor_run("run-paused-2", 2, &paused_previous.id);
    paused_successor.automation_id = AutomationId::from_string("automation-paused");
    assert!(run_repo
        .create_judge_successor_run(
            &AutomationId::from_string("automation-paused"),
            &paused_previous.id,
            paused_successor,
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_run_repo_rejects_skip_judge_successor_mismatches() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let previous = run(
        "run-1",
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    run_repo.create_run(previous.clone()).await.unwrap();

    let mut wrong_automation = successor_run("run-2", 2, &previous.id);
    wrong_automation.automation_id = AutomationId::from_string("automation-2");
    let error = run_repo
        .skip_judge_and_create_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            wrong_automation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));

    let mut wrong_base = successor_run("run-3", 3, &previous.id);
    wrong_base.base_from_run_id = Some(AutomationRunId::from_string("other-run"));
    let error = run_repo
        .skip_judge_and_create_successor_run(
            &AutomationId::from_string("automation-1"),
            &previous.id,
            wrong_base,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));
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

    let lease_expires_at = Utc::now() + chrono::Duration::minutes(3);
    assert!(run_repo
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

    let updated = run_repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(updated.judge_state, AutomationJudgeState::InProgress);
    assert_eq!(updated.judge_verdict_json, None);
    assert_eq!(updated.error_detail, None);
    assert_eq!(updated.judge_lease_expires_at, Some(lease_expires_at));

    let stale_lease = lease_expires_at + chrono::Duration::minutes(1);
    assert!(!run_repo
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
    let still_in_progress = run_repo.get_by_id(&failed.id).await.unwrap().unwrap();
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

    assert!(run_repo
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
    let completed = run_repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(completed.judge_state, AutomationJudgeState::Done);
    assert_eq!(
        completed.judge_verdict_json.as_deref(),
        Some(r#"{"decision":"stop"}"#)
    );
    assert_eq!(completed.judge_model_id.as_deref(), Some("haiku"));
    assert_eq!(completed.judge_lease_expires_at, None);
}

#[tokio::test]
async fn sqlite_run_repo_round_trips_and_updates_plan_gate_fields() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let agent_phase_started_at = Utc::now();
    let mut run = run(
        "run-1",
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
    run_repo.create_run(run.clone()).await.unwrap();

    let stored = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(stored, run);

    assert!(run_repo
        .compare_and_swap_plan_judge_state(
            &run.id,
            AutomationPlanJudgeState::InProgress,
            AutomationPlanJudgeState::Done,
            Some(r#"{"decision":"approve"}"#.to_string()),
            None,
        )
        .await
        .unwrap());
    let updated = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(updated.plan_judge_state, AutomationPlanJudgeState::Done);
    assert_eq!(
        updated.plan_judge_verdict_json.as_deref(),
        Some(r#"{"decision":"approve"}"#)
    );
    assert_eq!(updated.plan_judge_lease_expires_at, None);

    assert!(run_repo
        .set_plan_pending_instructions(&run.id, None)
        .await
        .unwrap()
        .unwrap()
        .plan_pending_instructions
        .is_none());
    assert_eq!(
        run_repo
            .set_plan_revision_round(&run.id, 3)
            .await
            .unwrap()
            .unwrap()
            .plan_revision_round,
        3
    );
    assert_eq!(
        run_repo
            .set_plan_last_parked_artifact_id(&run.id, Some("artifact-plan-2".to_string()))
            .await
            .unwrap()
            .unwrap()
            .plan_last_parked_artifact_id
            .as_deref(),
        Some("artifact-plan-2")
    );
    assert_eq!(
        run_repo
            .set_plan_reminder_count(&run.id, 2)
            .await
            .unwrap()
            .unwrap()
            .plan_reminder_count,
        2
    );
    let new_phase_started_at = Utc::now() + chrono::Duration::minutes(1);
    assert_eq!(
        run_repo
            .set_agent_phase_started_at(&run.id, Some(new_phase_started_at))
            .await
            .unwrap()
            .unwrap()
            .agent_phase_started_at,
        Some(new_phase_started_at)
    );
}

#[tokio::test]
async fn sqlite_run_repo_status_cas_with_agent_phase_started_at_uses_observed_phase() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let run = run(
        "run-1",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();
    let observed_phase_started_at = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();

    assert!(run_repo
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

    let updated = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(updated.status, AutomationRunStatus::Running);
    assert_eq!(
        updated.agent_phase_started_at,
        Some(observed_phase_started_at)
    );
}

#[tokio::test]
async fn sqlite_run_repo_clearing_pending_instructions_cas_sets_running_phase() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let mut run = run(
        "run-pending-plan",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_pending_instructions = Some("Revise the rollout risks.".to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    assert!(run_repo
        .compare_and_swap_status_clearing_plan_pending_instructions(
            &run.id,
            AutomationRunStatus::AwaitingPlanApproval,
            AutomationRunStatus::Running,
            None,
            None,
        )
        .await
        .unwrap());

    let running = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(running.status, AutomationRunStatus::Running);
    assert_eq!(running.plan_pending_instructions, None);
    assert!(running.agent_phase_started_at.is_some());
    assert_eq!(running.finished_at, None);

    assert!(!run_repo
        .compare_and_swap_status_clearing_plan_pending_instructions(
            &run.id,
            AutomationRunStatus::AwaitingPlanApproval,
            AutomationRunStatus::Completed,
            Some("late".to_string()),
            Some("stale transition".to_string()),
        )
        .await
        .unwrap());

    let unchanged = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(unchanged.status, AutomationRunStatus::Running);
    assert_eq!(unchanged.error_code, None);
    assert_eq!(unchanged.error_detail, None);
}

#[tokio::test]
async fn sqlite_run_repo_publication_metadata_can_clear_and_error_is_published_only() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let run = run(
        "run-publication",
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    let with_pr = run_repo
        .update_publication_metadata(
            &run.id,
            AutomationRunPublicationMetadata {
                pr_number: Some(647),
                pr_url: Some("https://github.com/aigentive/ralphx.app/pull/647".to_string()),
                pr_title: Some("Automation run".to_string()),
                pr_head_ref_name: Some("automation/run-647".to_string()),
                pr_base_ref_name: Some("main".to_string()),
            },
        )
        .await
        .unwrap()
        .expect("running run accepts publication metadata");
    assert_eq!(with_pr.pr_number, Some(647));

    let cleared = run_repo
        .clear_publication_metadata(&run.id)
        .await
        .unwrap()
        .expect("existing run clears publication metadata");
    assert_eq!(cleared.pr_number, None);
    assert_eq!(cleared.pr_url, None);
    assert_eq!(cleared.pr_title, None);
    assert_eq!(cleared.pr_head_ref_name, None);
    assert_eq!(cleared.pr_base_ref_name, None);
    assert!(run_repo
        .clear_publication_metadata(&AutomationRunId::from_string("missing-run"))
        .await
        .unwrap()
        .is_none());

    assert!(run_repo
        .compare_and_swap_status(
            &run.id,
            AutomationRunStatus::Running,
            AutomationRunStatus::Published,
            None,
            None,
        )
        .await
        .unwrap());
    let failed_status_check = run_repo
        .update_published_run_error(
            &run.id,
            Some("checks_failed".to_string()),
            Some("Required status check failed".to_string()),
        )
        .await
        .unwrap()
        .expect("published run stores merge-blocking error");
    assert_eq!(
        failed_status_check.error_code.as_deref(),
        Some("checks_failed")
    );
    assert_eq!(
        failed_status_check.error_detail.as_deref(),
        Some("Required status check failed")
    );

    let cleared_error = run_repo
        .update_published_run_error(&run.id, None, None)
        .await
        .unwrap()
        .expect("published run clears merge-blocking error");
    assert_eq!(cleared_error.error_code, None);
    assert_eq!(cleared_error.error_detail, None);
    assert!(run_repo
        .update_published_run_error(
            &AutomationRunId::from_string("missing-run"),
            Some("missing".to_string()),
            None,
        )
        .await
        .unwrap()
        .is_none());

    assert!(run_repo
        .compare_and_swap_status(
            &run.id,
            AutomationRunStatus::Published,
            AutomationRunStatus::Completed,
            None,
            None,
        )
        .await
        .unwrap());
    assert!(run_repo
        .update_published_run_error(&run.id, Some("late".to_string()), None)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_run_repo_plan_gate_clear_and_missing_setters_are_guarded() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let mut run = run(
        "run-plan-clear",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::InProgress;
    run.plan_judge_lease_expires_at = Some(lease_expires_at);
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    assert!(run_repo.clear_plan_judge_state(&run.id).await.unwrap());
    let cleared = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(cleared.plan_judge_state, AutomationPlanJudgeState::None);
    assert_eq!(cleared.plan_judge_lease_expires_at, None);
    assert_eq!(cleared.plan_judge_verdict_json, None);
    assert!(!run_repo.clear_plan_judge_state(&run.id).await.unwrap());

    let missing = AutomationRunId::from_string("missing-run");
    assert!(run_repo
        .set_plan_pending_instructions(&missing, Some("revise".to_string()))
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .set_plan_revision_round(&missing, 9)
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .set_plan_last_parked_artifact_id(&missing, Some("artifact-1".to_string()))
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .set_plan_reminder_count(&missing, 3)
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .set_agent_phase_started_at(&missing, Some(Utc::now()))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_plan_judge_cas_rejects_wrong_from_without_mutating_fields() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let mut run = run(
        "run-plan-cas-stale",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::InProgress;
    run.plan_judge_lease_expires_at = Some(lease_expires_at);
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    assert!(!run_repo
        .compare_and_swap_plan_judge_state(
            &run.id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::Done,
            Some(r#"{"decision":"approve"}"#.to_string()),
            None,
        )
        .await
        .unwrap());

    let unchanged = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
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
async fn sqlite_plan_judge_dispatch_sets_lease_and_preserves_stored_verdict() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(5);
    let mut run = run(
        "run-plan-cas-dispatch",
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    assert!(run_repo
        .compare_and_swap_plan_judge_state(
            &run.id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::InProgress,
            None,
            Some(lease_expires_at),
        )
        .await
        .unwrap());

    let dispatched = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
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
async fn sqlite_automation_repo_deletes_attachments_and_context_refs() {
    let (db, project_id, automation_repo, _run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-cleanup",
            project_id.clone(),
            AutomationStatus::Stopped,
        ))
        .await
        .unwrap();
    // Second automation so the "other" child rows satisfy the FK constraint and
    // prove the deletes are scoped by automation_id.
    automation_repo
        .create(automation(
            "automation-other",
            project_id,
            AutomationStatus::Stopped,
        ))
        .await
        .unwrap();

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO automation_attachments (id, automation_id, file_name, file_path, created_at)
             VALUES ('att-1', 'automation-cleanup', 'a.md', '/tmp/a.md', '2026-07-07T00:00:00+00:00'),
                    ('att-2', 'automation-cleanup', 'b.md', '/tmp/b.md', '2026-07-07T00:00:00+00:00'),
                    ('att-other', 'automation-other', 'c.md', '/tmp/c.md', '2026-07-07T00:00:00+00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO automation_context_refs (id, automation_id, ref_kind, payload_json, position)
             VALUES ('ref-1', 'automation-cleanup', 'project', '{}', 0),
                    ('ref-other', 'automation-other', 'project', '{}', 0)",
            [],
        )
        .unwrap();
    });

    let automation_id = AutomationId::from_string("automation-cleanup");
    assert_eq!(
        automation_repo
            .delete_attachments_for_automation(&automation_id)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        automation_repo
            .delete_context_refs_for_automation(&automation_id)
            .await
            .unwrap(),
        1
    );

    // Rows for other automations are untouched.
    db.with_connection(|conn| {
        let attachments: i64 = conn
            .query_row("SELECT COUNT(*) FROM automation_attachments", [], |row| {
                row.get(0)
            })
            .unwrap();
        let refs: i64 = conn
            .query_row("SELECT COUNT(*) FROM automation_context_refs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(attachments, 1);
        assert_eq!(refs, 1);
    });
}

#[tokio::test]
async fn sqlite_find_run_by_conversation_id_returns_latest_linked_run() {
    let (db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id.clone(),
            AutomationStatus::Active,
        ))
        .await
        .unwrap();

    let first = run(
        "run-1",
        1,
        AutomationRunStatus::Provisioning,
        AutomationJudgeState::None,
    );
    run_repo.create_run(first.clone()).await.unwrap();

    let conversation_id = ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id;
    conversation.automation_id = Some(AutomationId::from_string("automation-1"));
    conversation.automation_run_id = Some(first.id.clone());
    db.insert_conversation(conversation);

    run_repo
        .update_start_metadata(&first.id, &conversation_id, None)
        .await
        .unwrap()
        .expect("run should link conversation");

    let found = run_repo
        .find_run_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("linked run should be found");
    assert_eq!(found.id, first.id);
    assert_eq!(found.conversation_id, Some(conversation_id));

    assert!(run_repo
        .find_run_by_conversation_id(&ChatConversationId::from_string(
            "44444444-4444-4444-4444-444444444444"
        ))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_run_repo_round_trips_goal_item_id() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    automation_repo
        .create(automation(
            "automation-1",
            project_id,
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let mut mapped = run(
        "run-1",
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    mapped.goal_item_id = Some("item-b1".to_string());
    run_repo.create_run(mapped.clone()).await.unwrap();
    let mut unmapped = run(
        "run-2",
        2,
        AutomationRunStatus::Pending,
        AutomationJudgeState::None,
    );
    unmapped.goal_item_id = None;
    run_repo.create_run(unmapped.clone()).await.unwrap();

    let stored_mapped = run_repo.get_by_id(&mapped.id).await.unwrap().unwrap();
    assert_eq!(stored_mapped.goal_item_id.as_deref(), Some("item-b1"));
    assert_eq!(stored_mapped, mapped);
    let stored_unmapped = run_repo.get_by_id(&unmapped.id).await.unwrap().unwrap();
    assert_eq!(stored_unmapped.goal_item_id, None);
}

#[tokio::test]
async fn sqlite_run_repo_delete_run_if_deletable_accepts_each_deletable_status() {
    for (suffix, status, judge_state) in [
        (
            "agent-failed",
            AutomationRunStatus::AgentFailed,
            AutomationJudgeState::Done,
        ),
        (
            "cancelled",
            AutomationRunStatus::Cancelled,
            AutomationJudgeState::Failed,
        ),
    ] {
        let (_db, project_id, automation_repo, run_repo) = setup_repos();
        let automation = automation("automation-1", project_id, AutomationStatus::Stopped);
        automation_repo.create(automation.clone()).await.unwrap();
        let run = run(&format!("run-{suffix}"), 1, status, judge_state);
        run_repo.create_run(run.clone()).await.unwrap();

        assert_eq!(
            run_repo
                .delete_run_if_deletable(&automation.id, &run.id)
                .await
                .unwrap(),
            1
        );
        assert!(run_repo.get_by_id(&run.id).await.unwrap().is_none());
        assert!(run_repo
            .list_for_automation(&automation.id)
            .await
            .unwrap()
            .is_empty());
    }
}

#[tokio::test]
async fn sqlite_run_repo_delete_run_if_deletable_rejects_each_guard_without_deleting() {
    for (suffix, status, judge_state, add_newer, wrong_automation, wrong_run) in [
        (
            "status",
            AutomationRunStatus::Running,
            AutomationJudgeState::None,
            false,
            false,
            false,
        ),
        (
            "judge",
            AutomationRunStatus::AgentFailed,
            AutomationJudgeState::InProgress,
            false,
            false,
            false,
        ),
        (
            "not-latest",
            AutomationRunStatus::AgentFailed,
            AutomationJudgeState::Done,
            true,
            false,
            false,
        ),
        (
            "automation-id",
            AutomationRunStatus::AgentFailed,
            AutomationJudgeState::Done,
            false,
            true,
            false,
        ),
        (
            "run-id",
            AutomationRunStatus::AgentFailed,
            AutomationJudgeState::Done,
            false,
            false,
            true,
        ),
    ] {
        let (_db, project_id, automation_repo, run_repo) = setup_repos();
        let automation = automation("automation-1", project_id, AutomationStatus::Stopped);
        automation_repo.create(automation.clone()).await.unwrap();
        let target = run(&format!("run-{suffix}"), 1, status, judge_state);
        run_repo.create_run(target.clone()).await.unwrap();
        let newer = add_newer.then(|| {
            run(
                "run-newer",
                2,
                AutomationRunStatus::Completed,
                AutomationJudgeState::Done,
            )
        });
        if let Some(newer) = newer.as_ref() {
            run_repo.create_run(newer.clone()).await.unwrap();
        }
        let automation_id = if wrong_automation {
            AutomationId::from_string("automation-other")
        } else {
            automation.id.clone()
        };
        let run_id = if wrong_run {
            AutomationRunId::from_string("run-other")
        } else {
            target.id.clone()
        };

        assert_eq!(
            run_repo
                .delete_run_if_deletable(&automation_id, &run_id)
                .await
                .unwrap(),
            0,
            "{suffix} guard must reject the delete"
        );
        assert_eq!(
            run_repo.get_by_id(&target.id).await.unwrap(),
            Some(target),
            "{suffix} guard must leave the target row untouched"
        );
        if let Some(newer) = newer {
            assert_eq!(run_repo.get_by_id(&newer.id).await.unwrap(), Some(newer));
        }
    }
}

#[tokio::test]
async fn sqlite_run_repo_reopen_resets_judge_and_finished_fields_only() {
    let (_db, project_id, automation_repo, run_repo) = setup_repos();
    let automation = automation("automation-1", project_id, AutomationStatus::Stopped);
    automation_repo.create(automation).await.unwrap();
    let finished_at = Utc.with_ymd_and_hms(2026, 7, 23, 9, 0, 0).unwrap();
    let mut failed = run(
        "run-reopen-reset",
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    failed.judge_verdict_json = Some(r#"{"decision":"stop"}"#.to_string());
    failed.judge_model_id = Some("judge-model".to_string());
    failed.finished_at = Some(finished_at);
    run_repo.create_run(failed.clone()).await.unwrap();

    run_repo.clear_judge_state(&failed.id).await.unwrap();
    run_repo.clear_finished_at(&failed.id).await.unwrap();

    let reset = run_repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(reset.status, AutomationRunStatus::AgentFailed);
    assert_eq!(reset.judge_state, AutomationJudgeState::None);
    assert!(reset.judge_verdict_json.is_none());
    assert_eq!(reset.judge_model_id.as_deref(), Some("judge-model"));
    assert!(reset.finished_at.is_none());

    let missing = AutomationRunId::from_string("missing-run");
    run_repo.clear_judge_state(&missing).await.unwrap();
    run_repo.clear_finished_at(&missing).await.unwrap();
    assert!(run_repo.get_by_id(&missing).await.unwrap().is_none());
}
