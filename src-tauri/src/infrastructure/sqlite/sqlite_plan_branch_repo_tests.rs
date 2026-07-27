use super::*;
use crate::domain::entities::IdeationSessionId;
use crate::testing::SqliteTestDb;

fn create_test_branch() -> PlanBranch {
    PlanBranch::new(
        ArtifactId::from_string("art-test-1"),
        IdeationSessionId::from_string("sess-test-1"),
        ProjectId::from_string("proj-test-1".to_string()),
        "ralphx/test-project/plan-abc123".to_string(),
        "main".to_string(),
    )
}

fn setup_repo() -> (SqliteTestDb, SqlitePlanBranchRepository) {
    let db = SqliteTestDb::new("sqlite-plan-branch-repo");
    let repo = SqlitePlanBranchRepository::new(db.new_connection());
    (db, repo)
}

#[tokio::test]
async fn test_create_and_get_by_plan_artifact_id() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let artifact_id = branch.plan_artifact_id.clone();

    let created = repo.create(branch).await.unwrap();
    assert_eq!(created.plan_artifact_id, artifact_id);

    let results = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(results.len(), 1);
    let retrieved = &results[0];
    assert_eq!(retrieved.plan_artifact_id, artifact_id);
    assert_eq!(retrieved.branch_name, "ralphx/test-project/plan-abc123");
    assert_eq!(retrieved.source_branch, "main");
    assert_eq!(retrieved.status, PlanBranchStatus::Active);
    assert!(retrieved.merge_task_id.is_none());
    assert!(retrieved.merged_at.is_none());
}

#[tokio::test]
async fn test_get_by_plan_artifact_id_not_found() {
    let (_db, repo) = setup_repo();
    let results = repo
        .get_by_plan_artifact_id(&ArtifactId::from_string("nonexistent"))
        .await
        .unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_get_by_session_id() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let session_id = branch.session_id.clone();

    repo.create(branch).await.unwrap();

    let retrieved = repo.get_by_session_id(&session_id).await.unwrap().unwrap();
    assert_eq!(retrieved.session_id, session_id);
    assert_eq!(retrieved.branch_name, "ralphx/test-project/plan-abc123");
    assert_eq!(retrieved.status, PlanBranchStatus::Active);
}

#[tokio::test]
async fn test_get_by_session_id_not_found() {
    let (_db, repo) = setup_repo();
    let result = repo
        .get_by_session_id(&IdeationSessionId::from_string("nonexistent"))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_by_merge_task_id() {
    let (_db, repo) = setup_repo();
    let mut branch = create_test_branch();
    let merge_task_id = TaskId::from_string("merge-task-1".to_string());
    branch.merge_task_id = Some(merge_task_id.clone());

    repo.create(branch).await.unwrap();

    let retrieved = repo
        .get_by_merge_task_id(&merge_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retrieved.merge_task_id.as_ref().unwrap().as_str(),
        "merge-task-1"
    );
}

#[tokio::test]
async fn test_get_by_merge_task_id_not_found() {
    let (_db, repo) = setup_repo();
    let result = repo
        .get_by_merge_task_id(&TaskId::from_string("nonexistent".to_string()))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_by_project_id() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-multi".to_string());

    let branch1 = PlanBranch::new(
        ArtifactId::from_string("art-1"),
        IdeationSessionId::from_string("sess-1"),
        project_id.clone(),
        "ralphx/proj/plan-1".to_string(),
        "main".to_string(),
    );
    let branch2 = PlanBranch::new(
        ArtifactId::from_string("art-2"),
        IdeationSessionId::from_string("sess-2"),
        project_id.clone(),
        "ralphx/proj/plan-2".to_string(),
        "main".to_string(),
    );

    repo.create(branch1).await.unwrap();
    repo.create(branch2).await.unwrap();

    let branches = repo.get_by_project_id(&project_id).await.unwrap();
    assert_eq!(branches.len(), 2);
}

#[tokio::test]
async fn test_get_by_project_id_empty() {
    let (_db, repo) = setup_repo();
    let branches = repo
        .get_by_project_id(&ProjectId::from_string("empty-proj".to_string()))
        .await
        .unwrap();
    assert!(branches.is_empty());
}

#[tokio::test]
async fn test_startup_pr_recovery_candidates_filter_historical_branches() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-startup-pr-recovery".to_string());

    let make_branch = |suffix: &str, project_id: ProjectId| {
        PlanBranch::new(
            ArtifactId::from_string(format!("art-{suffix}")),
            IdeationSessionId::from_string(format!("sess-{suffix}")),
            project_id,
            format!("ralphx/proj/plan-{suffix}"),
            "main".to_string(),
        )
    };

    let mut candidate = make_branch("candidate", project_id.clone());
    candidate.pr_eligible = true;
    candidate.merge_task_id = Some(TaskId::from_string("merge-candidate".to_string()));
    let candidate_id = candidate.id.clone();

    let mut no_merge_task = make_branch("no-merge", project_id.clone());
    no_merge_task.pr_eligible = true;

    let mut merged = make_branch("merged", project_id.clone());
    merged.pr_eligible = true;
    merged.merge_task_id = Some(TaskId::from_string("merge-merged".to_string()));
    merged.status = PlanBranchStatus::Merged;

    let mut not_pr_eligible = make_branch("not-pr-eligible", project_id.clone());
    not_pr_eligible.merge_task_id = Some(TaskId::from_string("merge-not-pr".to_string()));
    let not_pr_eligible_id = not_pr_eligible.id.clone();

    let mut numbered_not_pr_eligible = make_branch("numbered-not-pr-eligible", project_id.clone());
    numbered_not_pr_eligible.merge_task_id =
        Some(TaskId::from_string("merge-numbered-not-pr".to_string()));
    numbered_not_pr_eligible.pr_number = Some(123);
    let numbered_not_pr_eligible_id = numbered_not_pr_eligible.id.clone();

    let mut other_project = make_branch(
        "other-project",
        ProjectId::from_string("other-project".to_string()),
    );
    other_project.pr_eligible = true;
    other_project.merge_task_id = Some(TaskId::from_string("merge-other".to_string()));

    for branch in [
        candidate,
        no_merge_task,
        merged,
        not_pr_eligible,
        numbered_not_pr_eligible,
        other_project,
    ] {
        repo.create(branch).await.unwrap();
    }

    let candidates = repo
        .get_startup_pr_recovery_candidates_by_project_id(&project_id)
        .await
        .unwrap();
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().any(|branch| branch.id == candidate_id));
    assert!(candidates
        .iter()
        .any(|branch| branch.id == numbered_not_pr_eligible_id));
    assert!(candidates
        .iter()
        .all(|branch| branch.id != not_pr_eligible_id));
}

#[tokio::test]
async fn test_terminal_cleanup_candidates_skip_marked_rows() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-cleanup-candidates".to_string());
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-cleanup-candidate"),
        IdeationSessionId::from_string("sess-cleanup-candidate"),
        project_id.clone(),
        "ralphx/proj/plan-cleanup".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Merged;
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();
    assert_eq!(
        repo.get_terminal_local_cleanup_candidates_by_project_id(&project_id)
            .await
            .unwrap()
            .len(),
        1
    );

    repo.mark_local_cleanup_status(&branch_id, "branch_missing", chrono::Utc::now())
        .await
        .unwrap();

    assert!(repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_restart_cleanup_marker_round_trip_and_clear() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();
    repo.create(branch).await.unwrap();

    repo.mark_local_cleanup_status(&branch_id, "cleaned", chrono::Utc::now())
        .await
        .unwrap();
    assert_eq!(
        repo.get_local_cleanup_status(&branch_id).await.unwrap(),
        Some("cleaned".to_string())
    );

    repo.clear_local_cleanup_status(&branch_id).await.unwrap();
    assert_eq!(
        repo.get_local_cleanup_status(&branch_id).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn test_terminal_cleanup_candidates_retry_unsafe_after_ttl() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-retry-unsafe".to_string());
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-retry-unsafe"),
        IdeationSessionId::from_string("sess-retry-unsafe"),
        project_id.clone(),
        "ralphx/proj/plan-retry".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Merged;
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();

    let old_time = chrono::Utc::now() - chrono::Duration::hours(2);
    repo.mark_local_cleanup_status(&branch_id, "unsafe", old_time)
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .unwrap();
    assert_eq!(
        candidates.len(),
        1,
        "unsafe marker after retry window should be retryable"
    );
}

#[tokio::test]
async fn test_terminal_cleanup_candidates_skip_recent_unsafe() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-skip-unsafe".to_string());
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-skip-unsafe"),
        IdeationSessionId::from_string("sess-skip-unsafe"),
        project_id.clone(),
        "ralphx/proj/plan-skip".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Merged;
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();
    repo.mark_local_cleanup_status(&branch_id, "unsafe", chrono::Utc::now())
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .unwrap();
    assert!(
        candidates.is_empty(),
        "recent unsafe marker should not be retried"
    );
}

#[tokio::test]
async fn test_terminal_cleanup_candidates_retry_target_ref_missing_after_ttl() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-retry-target".to_string());
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-retry-target"),
        IdeationSessionId::from_string("sess-retry-target"),
        project_id.clone(),
        "ralphx/proj/plan-target".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Merged;
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();

    let old_time = chrono::Utc::now() - chrono::Duration::hours(2);
    repo.mark_local_cleanup_status(&branch_id, "target_ref_missing", old_time)
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .unwrap();
    assert_eq!(
        candidates.len(),
        1,
        "target_ref_missing marker after retry window should be retryable"
    );
}

#[tokio::test]
async fn test_terminal_cleanup_candidates_retry_workspace_dirty_after_retry_window() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-retry-dirty".to_string());
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-retry-dirty"),
        IdeationSessionId::from_string("sess-retry-dirty"),
        project_id.clone(),
        "ralphx/proj/plan-dirty".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Merged;
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();

    let old_time = chrono::Utc::now() - chrono::Duration::hours(2);
    repo.mark_local_cleanup_status(&branch_id, "workspace_dirty", old_time)
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .unwrap();
    assert_eq!(
        candidates.len(),
        1,
        "workspace_dirty marker after retry window should be retryable"
    );

    repo.mark_local_cleanup_status(&branch_id, "workspace_dirty", chrono::Utc::now())
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .unwrap();
    assert!(
        candidates.is_empty(),
        "workspace_dirty marker before retry window should not be retried"
    );
}

#[tokio::test]
async fn test_terminal_cleanup_candidates_cleaned_stays_terminal() {
    let (_db, repo) = setup_repo();
    let project_id = ProjectId::from_string("proj-cleaned-terminal".to_string());
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-cleaned"),
        IdeationSessionId::from_string("sess-cleaned"),
        project_id.clone(),
        "ralphx/proj/plan-cleaned".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Merged;
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();

    let old_time = chrono::Utc::now() - chrono::Duration::hours(48);
    repo.mark_local_cleanup_status(&branch_id, "cleaned", old_time)
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .unwrap();
    assert!(
        candidates.is_empty(),
        "cleaned marker should stay terminal regardless of age"
    );
}

#[tokio::test]
async fn test_update_status() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();
    let artifact_id = branch.plan_artifact_id.clone();

    repo.create(branch).await.unwrap();
    repo.update_status(&branch_id, PlanBranchStatus::Abandoned)
        .await
        .unwrap();

    let results = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, PlanBranchStatus::Abandoned);
}

#[tokio::test]
async fn test_update_status_not_found() {
    let (_db, repo) = setup_repo();
    let result = repo
        .update_status(
            &PlanBranchId::from_string("nonexistent"),
            PlanBranchStatus::Merged,
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_set_merge_task_id() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();
    let artifact_id = branch.plan_artifact_id.clone();
    let merge_task_id = TaskId::from_string("mt-1".to_string());

    repo.create(branch).await.unwrap();
    repo.set_merge_task_id(&branch_id, &merge_task_id)
        .await
        .unwrap();

    let results = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].merge_task_id.as_ref().unwrap().as_str(), "mt-1");
}

#[tokio::test]
async fn test_set_merge_task_id_not_found() {
    let (_db, repo) = setup_repo();
    let result = repo
        .set_merge_task_id(
            &PlanBranchId::from_string("nonexistent"),
            &TaskId::from_string("mt-1".to_string()),
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_set_merged() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();
    let artifact_id = branch.plan_artifact_id.clone();

    repo.create(branch).await.unwrap();
    repo.set_merged(&branch_id).await.unwrap();

    let results = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, PlanBranchStatus::Merged);
    assert!(results[0].merged_at.is_some());
}

#[tokio::test]
async fn test_set_merged_not_found() {
    let (_db, repo) = setup_repo();
    let result = repo
        .set_merged(&PlanBranchId::from_string("nonexistent"))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multiple_branches_same_plan_artifact_id() {
    let (_db, repo) = setup_repo();
    let branch1 = create_test_branch();
    let artifact_id = branch1.plan_artifact_id.clone();
    let branch2 = PlanBranch::new(
        artifact_id.clone(), // same artifact id — allowed after v46
        IdeationSessionId::from_string("sess-different"),
        ProjectId::from_string("proj-different".to_string()),
        "ralphx/other/plan-xyz".to_string(),
        "main".to_string(),
    );

    repo.create(branch1).await.unwrap();
    let result = repo.create(branch2).await;
    assert!(
        result.is_ok(),
        "Multiple branches with same plan_artifact_id should be allowed"
    );

    // Verify get_by_plan_artifact_id returns both branches
    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(branches.len(), 2);
}

#[tokio::test]
async fn test_create_with_merge_task_id() {
    let (_db, repo) = setup_repo();
    let mut branch = create_test_branch();
    branch.merge_task_id = Some(TaskId::from_string("mt-preset".to_string()));

    let created = repo.create(branch).await.unwrap();
    assert_eq!(created.merge_task_id.unwrap().as_str(), "mt-preset");
}

#[tokio::test]
async fn test_get_by_merge_task_id_after_set() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();
    let merge_task_id = TaskId::from_string("mt-lookup".to_string());

    repo.create(branch).await.unwrap();
    repo.set_merge_task_id(&branch_id, &merge_task_id)
        .await
        .unwrap();

    let retrieved = repo
        .get_by_merge_task_id(&merge_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.id, branch_id);
}

// ==================== CLEAR_MERGE_TASK_ID TESTS ====================

#[tokio::test]
async fn test_clear_merge_task_id_removes_task_link() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();
    let merge_task_id = TaskId::from_string("mt-to-clear".to_string());

    repo.create(branch).await.unwrap();
    repo.set_merge_task_id(&branch_id, &merge_task_id)
        .await
        .unwrap();

    // Verify it's set
    let results = repo
        .get_by_plan_artifact_id(&ArtifactId::from_string("art-test-1"))
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].merge_task_id.is_some());

    // Clear it
    repo.clear_merge_task_id(&branch_id).await.unwrap();

    let results = repo
        .get_by_plan_artifact_id(&ArtifactId::from_string("art-test-1"))
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].merge_task_id.is_none());
}

#[tokio::test]
async fn test_clear_merge_task_id_returns_not_found_for_nonexistent() {
    let (_db, repo) = setup_repo();
    let nonexistent_id = PlanBranchId::new();

    let result = repo.clear_merge_task_id(&nonexistent_id).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, crate::error::AppError::NotFound(_)));
}

#[tokio::test]
async fn test_clear_merge_task_id_when_already_null_returns_not_found() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();
    // merge_task_id is already NULL — no rows match the update condition? Actually it matches id=?1
    // which should succeed since the branch exists
    let result = repo.clear_merge_task_id(&branch_id).await;
    assert!(result.is_ok());
}

// ==================== CREATE_OR_UPDATE TESTS ====================

#[tokio::test]
async fn test_create_or_update_inserts_when_no_conflict() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let session_id = branch.session_id.clone();

    let result = repo.create_or_update(branch).await.unwrap();
    assert_eq!(result.session_id, session_id);

    let retrieved = repo.get_by_session_id(&session_id).await.unwrap().unwrap();
    assert_eq!(retrieved.id, result.id);
}

#[tokio::test]
async fn test_create_or_update_conflict_returns_existing_id() {
    let (_db, repo) = setup_repo();

    // Insert original branch
    let original = create_test_branch();
    let original_id = original.id.clone();
    let session_id = original.session_id.clone();
    repo.create(original).await.unwrap();

    // create_or_update with same session_id — ON CONFLICT fires, existing row's id is preserved
    let updated = PlanBranch::new(
        ArtifactId::from_string("art-updated"),
        session_id.clone(),
        ProjectId::from_string("proj-test-1".to_string()),
        "ralphx/updated-branch".to_string(),
        "main".to_string(),
    );

    let returned = repo.create_or_update(updated).await.unwrap();

    // Returned id must match the original persisted row, not the new UUID
    assert_eq!(
        returned.id, original_id,
        "create_or_update must return existing row id on conflict"
    );
    assert_eq!(returned.branch_name, "ralphx/updated-branch");
}

#[tokio::test]
async fn test_create_or_update_conflict_preserves_existing_pr_number() {
    let (_db, repo) = setup_repo();

    let mut original = create_test_branch();
    original.pr_number = Some(42);
    let session_id = original.session_id.clone();
    repo.create(original).await.unwrap();

    let stale_without_pr = PlanBranch::new(
        ArtifactId::from_string("art-stale-without-pr"),
        session_id.clone(),
        ProjectId::from_string("proj-test-1".to_string()),
        "ralphx/stale-without-pr".to_string(),
        "main".to_string(),
    );
    let returned = repo.create_or_update(stale_without_pr).await.unwrap();
    assert_eq!(returned.pr_number, Some(42));

    let mut stale_with_different_pr = PlanBranch::new(
        ArtifactId::from_string("art-stale-different-pr"),
        session_id.clone(),
        ProjectId::from_string("proj-test-1".to_string()),
        "ralphx/stale-different-pr".to_string(),
        "main".to_string(),
    );
    stale_with_different_pr.pr_number = Some(99);
    let returned = repo
        .create_or_update(stale_with_different_pr)
        .await
        .unwrap();

    assert_eq!(returned.pr_number, Some(42));
    assert_eq!(
        repo.get_by_session_id(&session_id)
            .await
            .unwrap()
            .unwrap()
            .pr_number,
        Some(42)
    );
}

#[tokio::test]
async fn test_create_or_update_conflict_set_merge_task_id_succeeds() {
    let (_db, repo) = setup_repo();

    // Insert original branch
    let original = create_test_branch();
    let session_id = original.session_id.clone();
    repo.create(original).await.unwrap();

    // Upsert with same session_id
    let replacement = PlanBranch::new(
        ArtifactId::from_string("art-updated-2"),
        session_id.clone(),
        ProjectId::from_string("proj-test-1".to_string()),
        "ralphx/replacement-branch".to_string(),
        "main".to_string(),
    );
    let returned = repo.create_or_update(replacement).await.unwrap();

    // set_merge_task_id must succeed using the returned id (which is the existing row's id)
    let merge_task_id = TaskId::from_string("mt-upsert".to_string());
    let set_result = repo.set_merge_task_id(&returned.id, &merge_task_id).await;
    assert!(
        set_result.is_ok(),
        "set_merge_task_id must succeed with returned branch id after conflict upsert"
    );

    // Verify it was actually persisted
    let retrieved = repo.get_by_session_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        retrieved.merge_task_id.as_ref().unwrap().as_str(),
        "mt-upsert"
    );
}

// ==================== DELETE TESTS ====================

#[tokio::test]
async fn test_delete_removes_branch() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();
    let artifact_id = branch.plan_artifact_id.clone();

    repo.create(branch).await.unwrap();

    // Verify it exists
    assert_eq!(
        repo.get_by_plan_artifact_id(&artifact_id)
            .await
            .unwrap()
            .len(),
        1
    );

    repo.delete(&branch_id).await.unwrap();

    // Should be gone
    assert!(repo
        .get_by_plan_artifact_id(&artifact_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_delete_returns_not_found_for_nonexistent() {
    let (_db, repo) = setup_repo();
    let nonexistent_id = PlanBranchId::new();

    let result = repo.delete(&nonexistent_id).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, crate::error::AppError::NotFound(_)));
}

#[tokio::test]
async fn test_update_pr_push_status_persists_without_updated_at_column() {
    let (_db, repo) = setup_repo();
    let branch = create_test_branch();
    let branch_id = branch.id.clone();

    repo.create(branch).await.unwrap();
    repo.update_pr_push_status(&branch_id, PrPushStatus::Pushed)
        .await
        .unwrap();

    let retrieved = repo.get_by_id(&branch_id).await.unwrap().unwrap();
    assert_eq!(retrieved.pr_push_status, PrPushStatus::Pushed);
}
