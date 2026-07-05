use chrono::Utc;

use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversation, ChatConversationId,
    ProjectId,
};
use crate::domain::repositories::{
    AutomationRepository, AutomationRunPublicationMetadata, AutomationRunRepository,
    AutomationSettingsPatch,
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
        AutomationRunStatus::Merged,
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

    assert!(run_repo
        .compare_and_swap_judge_state(
            &failed.id,
            AutomationJudgeState::InProgress,
            AutomationJudgeState::Done,
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
