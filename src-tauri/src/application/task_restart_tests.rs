use super::*;
use crate::domain::entities::task_metadata::StopRetryingReason;
use crate::domain::entities::{
    AgentRun, AgentRunStatus, ChatContextType, ChatConversation, ProjectId, TaskStep,
    ValidationCacheDecision, ValidationCacheMetadata, ValidationCommandCategory,
    ValidationCommandResult, ValidationCommandSource, ValidationCommandStatus,
    ValidationContextType, ValidationPurpose, ValidationRun, ValidationRunMode,
    ValidationRunStatus,
};
use crate::domain::entities::{
    ExecutionRecoveryEvent, ExecutionRecoveryEventKind, ExecutionRecoveryReasonCode,
    ExecutionRecoverySource,
};
use crate::infrastructure::memory::{MemoryTaskRepository, MemoryTaskStepRepository};
use crate::utils::path_safety::validate_absolute_non_root_path;

fn git(path: &std::path::Path, args: &[&str]) {
    let path = validate_absolute_non_root_path(path, "task restart test git repository")
        .expect("test git repository path should be safe");
    let output = std::process::Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(&path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_test_dir(path: &std::path::Path) {
    let path = validate_absolute_non_root_path(path, "task restart test directory")
        .expect("test directory path should be safe");
    // codeql[rust/path-injection]
    std::fs::create_dir_all(&path).unwrap();
}

fn write_test_file(path: &std::path::Path, contents: &str) {
    let path = validate_absolute_non_root_path(path, "task restart test file")
        .expect("test file path should be safe");
    // codeql[rust/path-injection]
    std::fs::write(&path, contents).unwrap();
}

#[cfg(unix)]
fn symlink_test_path(target: &std::path::Path, link: &std::path::Path) {
    let target = validate_absolute_non_root_path(target, "task restart test symlink target")
        .expect("test symlink target should be safe");
    let link = validate_absolute_non_root_path(link, "task restart test symlink link")
        .expect("test symlink link should be safe");
    // codeql[rust/path-injection]
    std::os::unix::fs::symlink(&target, &link).unwrap();
}

async fn persist_failed_episode(
    state: &crate::application::AppState,
    task: &Task,
) -> chrono::DateTime<chrono::Utc> {
    state.task_repo.create(task.clone()).await.unwrap();
    state
        .task_repo
        .persist_status_change(
            &task.id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();
    state.task_repo.update(task).await.unwrap();
    state
        .task_repo
        .get_status_last_entered_at(&task.id, InternalStatus::Executing)
        .await
        .unwrap()
        .unwrap()
}

async fn add_completed_step(state: &crate::application::AppState, task_id: &TaskId) {
    let mut step = TaskStep::new(task_id.clone(), "done".to_string(), 0, "test".to_string());
    step.status = TaskStepStatus::Completed;
    state.task_step_repo.create(step).await.unwrap();
}

async fn add_task_execution_run(
    state: &crate::application::AppState,
    task_id: &TaskId,
    status: AgentRunStatus,
    started_at: chrono::DateTime<chrono::Utc>,
) -> AgentRun {
    let mut conversation = ChatConversation::new_task(task_id.clone());
    conversation.context_type = ChatContextType::TaskExecution;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut run = AgentRun::new(conversation.id);
    run.status = status;
    run.started_at = started_at;
    if status == AgentRunStatus::Completed {
        run.completed_at = Some(chrono::Utc::now());
    }
    state.agent_run_repo.create(run).await.unwrap()
}

fn first_warning_code(classification: &FailedRestartClassification) -> &str {
    match classification {
        FailedRestartClassification::RestartRequired(warnings)
        | FailedRestartClassification::Blocked(warnings) => warnings[0].code.as_str(),
        FailedRestartClassification::RecoverToReview(_) => "recover_to_review",
    }
}

#[tokio::test]
async fn failed_recovery_blocks_dirty_current_attempt_without_clearing_refs() {
    use crate::application::AppState;
    use crate::domain::entities::{
        AgentRun, AgentRunStatus, ChatContextType, ChatConversation, Project,
    };

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Recovery Classifier".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Preserve dirty work".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/recover".to_string());
    let worktree = project.task_worktree_path(task.id.as_str());
    create_test_dir(&worktree);
    git(&worktree, &["init", "-b", "task/recover"]);
    git(&worktree, &["config", "user.email", "test@example.com"]);
    git(&worktree, &["config", "user.name", "RalphX Test"]);
    write_test_file(&worktree.join("tracked.txt"), "initial\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "initial"]);
    task.worktree_path = Some(worktree.to_string_lossy().into_owned());
    let task_id = task.id.clone();
    state.task_repo.create(task.clone()).await.unwrap();
    state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();
    // The memory repository applies the transition while recording history; restore the
    // incident state so the classifier sees the same Failed row as production.
    state.task_repo.update(&task).await.unwrap();

    let mut step = TaskStep::new(task_id.clone(), "done".to_string(), 0, "test".to_string());
    step.status = TaskStepStatus::Completed;
    state.task_step_repo.create(step).await.unwrap();
    let mut conversation = ChatConversation::new_task(task_id.clone());
    conversation.context_type = ChatContextType::TaskExecution;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut run = AgentRun::new(conversation.id);
    run.status = AgentRunStatus::Completed;
    run.completed_at = Some(chrono::Utc::now());
    state.agent_run_repo.create(run).await.unwrap();

    write_test_file(&worktree.join("tracked.txt"), "uncommitted\n");
    let classification = classify_failed_restart(&state, &task).await;
    let FailedRestartClassification::Blocked(warnings) = classification else {
        panic!("dirty current attempt must block, got {classification:?}");
    };
    assert_eq!(warnings[0].code, "dirty_worktree");

    let stored = state.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(stored.internal_status, InternalStatus::Failed);
    assert_eq!(stored.task_branch.as_deref(), Some("task/recover"));
    assert_eq!(
        stored.worktree_path.as_deref(),
        task.worktree_path.as_deref()
    );
}

#[tokio::test]
async fn failed_recovery_blocks_when_execution_attempt_authority_is_missing() {
    use crate::application::AppState;

    let state = AppState::new_test();
    let project = crate::domain::entities::Project::new(
        "Missing attempt authority".to_string(),
        "/tmp/missing-attempt-authority".to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id, "Do not discard preserved work".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/preserved".to_string());
    task.worktree_path = Some("/tmp/preserved-worktree".to_string());
    task.merge_commit_sha = Some("preserved-sha".to_string());
    state.task_repo.create(task.clone()).await.unwrap();

    let classification = classify_failed_restart(&state, &task).await;
    let FailedRestartClassification::Blocked(warnings) = classification else {
        panic!("missing attempt authority must block, got {classification:?}");
    };
    assert_eq!(warnings[0].code, "missing_execution_episode");

    let stored = state.task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(stored.internal_status, InternalStatus::Failed);
    assert_eq!(stored.task_branch, task.task_branch);
    assert_eq!(stored.worktree_path, task.worktree_path);
    assert_eq!(stored.merge_commit_sha, task.merge_commit_sha);
}

#[tokio::test]
async fn failed_recovery_reports_early_authority_guards() {
    use crate::application::AppState;
    use crate::domain::entities::Project;

    let state = AppState::new_test();
    let project = Project::new(
        "Authority guards".to_string(),
        "/tmp/authority-guards".to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let ready_task = Task::new(project.id.clone(), "Wrong status".to_string());
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &ready_task).await),
        "task_not_failed"
    );

    let mut task = Task::new(project.id.clone(), "Missing conversation".to_string());
    task.internal_status = InternalStatus::Failed;
    persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "missing_execution_conversation"
    );

    let mut conversation = ChatConversation::new_task(task.id.clone());
    conversation.context_type = ChatContextType::TaskExecution;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "missing_agent_run"
    );
}

#[tokio::test]
async fn failed_recovery_requires_current_completed_agent_run() {
    use crate::application::AppState;
    use crate::domain::entities::Project;

    let state = AppState::new_test();
    let project = Project::new(
        "Agent run guards".to_string(),
        "/tmp/agent-run-guards".to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Running agent run".to_string());
    task.internal_status = InternalStatus::Failed;
    let episode_entered_at = persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    add_task_execution_run(
        &state,
        &task.id,
        AgentRunStatus::Completed,
        episode_entered_at - chrono::Duration::seconds(1),
    )
    .await;
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "agent_run_not_current"
    );

    let state = AppState::new_test();
    state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id, "Incomplete agent run".to_string());
    task.internal_status = InternalStatus::Failed;
    let episode_entered_at = persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    add_task_execution_run(
        &state,
        &task.id,
        AgentRunStatus::Running,
        episode_entered_at + chrono::Duration::milliseconds(1),
    )
    .await;
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "agent_run_not_completed"
    );
}

#[tokio::test]
async fn failed_recovery_reports_project_and_worktree_guards() {
    use crate::application::AppState;
    use crate::domain::entities::Project;

    let state = AppState::new_test();
    let mut missing_project_task = Task::new(ProjectId::new(), "Missing project".to_string());
    missing_project_task.internal_status = InternalStatus::Failed;
    let episode_entered_at = persist_failed_episode(&state, &missing_project_task).await;
    add_completed_step(&state, &missing_project_task.id).await;
    add_task_execution_run(
        &state,
        &missing_project_task.id,
        AgentRunStatus::Completed,
        episode_entered_at + chrono::Duration::milliseconds(1),
    )
    .await;
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &missing_project_task).await),
        "project_missing"
    );

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Worktree guards".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Missing worktree".to_string());
    task.internal_status = InternalStatus::Failed;
    let episode_entered_at = persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    add_task_execution_run(
        &state,
        &task.id,
        AgentRunStatus::Completed,
        episode_entered_at + chrono::Duration::milliseconds(1),
    )
    .await;
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "missing_worktree"
    );

    task.worktree_path = Some(
        project
            .task_worktree_path(task.id.as_str())
            .to_string_lossy()
            .into_owned(),
    );
    state.task_repo.update(&task).await.unwrap();
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "missing_task_branch"
    );

    task.task_branch = Some("task/expected".to_string());
    task.worktree_path = Some(root.path().join("wrong").to_string_lossy().into_owned());
    state.task_repo.update(&task).await.unwrap();
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "worktree_path_mismatch"
    );

    task.worktree_path = Some(
        project
            .task_worktree_path(task.id.as_str())
            .to_string_lossy()
            .into_owned(),
    );
    state.task_repo.update(&task).await.unwrap();
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "missing_worktree"
    );
}

#[tokio::test]
async fn failed_recovery_reports_git_and_validation_guards() {
    use crate::application::AppState;
    use crate::domain::entities::Project;

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Git guards".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Branch mismatch".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/expected".to_string());
    let worktree = project.task_worktree_path(task.id.as_str());
    create_test_dir(&worktree);
    git(&worktree, &["init", "-b", "task/actual"]);
    git(&worktree, &["config", "user.email", "test@example.com"]);
    git(&worktree, &["config", "user.name", "RalphX Test"]);
    write_test_file(&worktree.join("tracked.txt"), "base\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "base"]);
    task.worktree_path = Some(worktree.to_string_lossy().into_owned());
    let episode_entered_at = persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    add_task_execution_run(
        &state,
        &task.id,
        AgentRunStatus::Completed,
        episode_entered_at + chrono::Duration::milliseconds(1),
    )
    .await;
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "task_branch_mismatch"
    );

    task.task_branch = Some("task/actual".to_string());
    state.task_repo.update(&task).await.unwrap();
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "task_diff_not_recoverable"
    );

    let base_sha = GitService::get_head_sha(&worktree).await.unwrap();
    write_test_file(&worktree.join("tracked.txt"), "changed\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "change"]);
    task.task_branch_base_ref = Some("base".to_string());
    task.task_branch_base_sha = Some(base_sha);
    state.task_repo.update(&task).await.unwrap();
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "missing_validation_evidence"
    );

    let stale_validation = ValidationRun {
        id: "validation-stale".to_string(),
        task_id: task.id.clone(),
        project_id: project.id,
        purpose: ValidationPurpose::Final,
        context_type: ValidationContextType::Execution,
        requested_by_agent: Some("test".to_string()),
        status: ValidationRunStatus::Passed,
        mode: ValidationRunMode::ReuseOrRun,
        policy_enabled: true,
        head_sha: Some("stale".to_string()),
        start_content_fingerprint: None,
        validated_content_fingerprint: None,
        promoted_commit_sha: Some("stale".to_string()),
        base_ref: Some("base".to_string()),
        analysis_fingerprint: None,
        status_episode_entered_at: Some(episode_entered_at),
        started_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
    };
    state
        .validation_run_repo
        .create_run(&stale_validation)
        .await
        .unwrap();
    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "validation_evidence_not_current"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn failed_recovery_blocks_worktree_symlink_that_resolves_to_root() {
    use crate::application::AppState;
    use crate::domain::entities::Project;

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Root Escape".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Root escape".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/root-escape".to_string());
    let expected_worktree = project.task_worktree_path(task.id.as_str());
    create_test_dir(expected_worktree.parent().unwrap());
    symlink_test_path(root.path(), &expected_worktree);
    task.worktree_path = Some(expected_worktree.to_string_lossy().into_owned());
    let episode_entered_at = persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    add_task_execution_run(
        &state,
        &task.id,
        AgentRunStatus::Completed,
        episode_entered_at + chrono::Duration::milliseconds(1),
    )
    .await;

    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "worktree_root_escape"
    );
}

#[tokio::test]
async fn failed_recovery_blocks_when_git_branch_cannot_be_read() {
    use crate::application::AppState;
    use crate::domain::entities::Project;

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Unreadable Head".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "No HEAD".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/no-head".to_string());
    let worktree = project.task_worktree_path(task.id.as_str());
    create_test_dir(&worktree);
    git(&worktree, &["init", "-b", "task/no-head"]);
    task.worktree_path = Some(worktree.to_string_lossy().into_owned());
    let episode_entered_at = persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    add_task_execution_run(
        &state,
        &task.id,
        AgentRunStatus::Completed,
        episode_entered_at + chrono::Duration::milliseconds(1),
    )
    .await;

    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "task_branch_read_failed"
    );
}

#[tokio::test]
async fn failed_recovery_blocks_malformed_legacy_validation_cache() {
    use crate::application::AppState;
    use crate::domain::entities::Project;

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Malformed Cache".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Malformed cache".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/malformed-cache".to_string());
    let worktree = project.task_worktree_path(task.id.as_str());
    create_test_dir(&worktree);
    git(&worktree, &["init", "-b", "task/malformed-cache"]);
    git(&worktree, &["config", "user.email", "test@example.com"]);
    git(&worktree, &["config", "user.name", "RalphX Test"]);
    write_test_file(&worktree.join("tracked.txt"), "base\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "base"]);
    let base_sha = GitService::get_head_sha(&worktree).await.unwrap();
    write_test_file(&worktree.join("tracked.txt"), "changed\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "change"]);
    task.worktree_path = Some(worktree.to_string_lossy().into_owned());
    task.task_branch_base_ref = Some("base".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.metadata = Some(r#"{"validation_cache":{"version":"bad"}}"#.to_string());
    let episode_entered_at = persist_failed_episode(&state, &task).await;
    add_completed_step(&state, &task.id).await;
    add_task_execution_run(
        &state,
        &task.id,
        AgentRunStatus::Completed,
        episode_entered_at + chrono::Duration::milliseconds(1),
    )
    .await;

    assert_eq!(
        first_warning_code(&classify_failed_restart(&state, &task).await),
        "legacy_validation_cache_read_failed"
    );
}

#[tokio::test]
async fn failed_recovery_accepts_complete_current_attempt_proof() {
    use crate::application::AppState;
    use crate::domain::entities::{
        AgentRun, AgentRunStatus, ChatContextType, ChatConversation, Project,
    };

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Recovery proof".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Recover this attempt".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/recover-proof".to_string());
    let worktree = project.task_worktree_path(task.id.as_str());
    create_test_dir(&worktree);
    git(&worktree, &["init", "-b", "task/recover-proof"]);
    git(&worktree, &["config", "user.email", "test@example.com"]);
    git(&worktree, &["config", "user.name", "RalphX Test"]);
    write_test_file(&worktree.join("tracked.txt"), "base\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "base"]);
    let base_sha = GitService::get_head_sha(&worktree).await.unwrap();
    write_test_file(&worktree.join("tracked.txt"), "completed work\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "completed work"]);
    let promoted_sha = GitService::get_head_sha(&worktree).await.unwrap();
    task.worktree_path = Some(worktree.to_string_lossy().into_owned());
    task.task_branch_base_ref = Some("base".to_string());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    state.task_repo.create(task.clone()).await.unwrap();
    state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();
    state.task_repo.update(&task).await.unwrap();
    let episode_entered_at = state
        .task_repo
        .get_status_last_entered_at(&task_id, InternalStatus::Executing)
        .await
        .unwrap()
        .unwrap();

    let mut step = TaskStep::new(task_id.clone(), "done".to_string(), 0, "test".to_string());
    step.status = TaskStepStatus::Completed;
    state.task_step_repo.create(step).await.unwrap();
    let mut conversation = ChatConversation::new_task(task_id.clone());
    conversation.context_type = ChatContextType::TaskExecution;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut agent_run = AgentRun::new(conversation.id);
    agent_run.status = AgentRunStatus::Completed;
    agent_run.completed_at = Some(chrono::Utc::now());
    let agent_run = state.agent_run_repo.create(agent_run).await.unwrap();

    let validation_run = ValidationRun {
        id: "validation-current".to_string(),
        task_id: task_id.clone(),
        project_id: project.id.clone(),
        purpose: ValidationPurpose::Final,
        context_type: ValidationContextType::Execution,
        requested_by_agent: Some("test".to_string()),
        status: ValidationRunStatus::Passed,
        mode: ValidationRunMode::ReuseOrRun,
        policy_enabled: true,
        head_sha: Some(promoted_sha.clone()),
        start_content_fingerprint: None,
        validated_content_fingerprint: None,
        promoted_commit_sha: Some(promoted_sha.clone()),
        base_ref: Some("base".to_string()),
        analysis_fingerprint: None,
        status_episode_entered_at: Some(episode_entered_at),
        started_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
    };
    state
        .validation_run_repo
        .create_run(&validation_run)
        .await
        .unwrap();
    state
        .validation_run_repo
        .add_command_result(&ValidationCommandResult {
            id: "validation-command".to_string(),
            validation_run_id: validation_run.id.clone(),
            task_id: task_id.clone(),
            project_id: project.id,
            command_source: ValidationCommandSource::ProjectAnalysisRef,
            command_ref: Some("tests".to_string()),
            command: "cargo test".to_string(),
            cwd: worktree.to_string_lossy().into_owned(),
            label: Some("Tests".to_string()),
            category: ValidationCommandCategory::Test,
            reason: None,
            related_files: Vec::new(),
            cache_key: "validation-cache".to_string(),
            cache_decision: ValidationCacheDecision::Ran,
            status: ValidationCommandStatus::Passed,
            exit_code: Some(0),
            duration_ms: Some(1),
            stdout_snippet: None,
            stderr_snippet: None,
            stdout_log_path: None,
            stderr_log_path: None,
            launcher_kind: None,
            resolved_shell_path: None,
            head_sha: Some(promoted_sha.clone()),
            analysis_fingerprint: None,
            status_episode_entered_at: Some(episode_entered_at),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let classification = classify_failed_restart(&state, &task).await;
    let FailedRestartClassification::RecoverToReview(evidence) = classification else {
        panic!("complete current attempt must recover, got {classification:?}");
    };
    assert_eq!(evidence.agent_run_id, agent_run.id.as_str());
    assert_eq!(evidence.validation_run_id, validation_run.id);
    assert_eq!(evidence.promoted_commit_sha, promoted_sha);
}

#[tokio::test]
async fn failed_recovery_accepts_legacy_validation_cache_when_validation_run_is_absent() {
    use crate::application::AppState;
    use crate::domain::entities::{
        AgentRun, AgentRunStatus, ChatContextType, ChatConversation, Project,
    };

    let state = AppState::new_test();
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new(
        "Legacy recovery proof".to_string(),
        root.path().join("project").to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(root.path().to_string_lossy().into_owned());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Recover legacy proof".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/recover-legacy-proof".to_string());
    let worktree = project.task_worktree_path(task.id.as_str());
    create_test_dir(&worktree);
    git(&worktree, &["init", "-b", "task/recover-legacy-proof"]);
    git(&worktree, &["config", "user.email", "test@example.com"]);
    git(&worktree, &["config", "user.name", "RalphX Test"]);
    write_test_file(&worktree.join("tracked.txt"), "base\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "base"]);
    let base_sha = GitService::get_head_sha(&worktree).await.unwrap();
    write_test_file(&worktree.join("tracked.txt"), "completed legacy work\n");
    git(&worktree, &["add", "tracked.txt"]);
    git(&worktree, &["commit", "-m", "completed legacy work"]);
    let promoted_sha = GitService::get_head_sha(&worktree).await.unwrap();
    task.worktree_path = Some(worktree.to_string_lossy().into_owned());
    task.task_branch_base_ref = Some("base".to_string());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    state.task_repo.create(task.clone()).await.unwrap();
    state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();
    let episode_entered_at = state
        .task_repo
        .get_status_last_entered_at(&task_id, InternalStatus::Executing)
        .await
        .unwrap()
        .unwrap();
    let cache = ValidationCacheMetadata {
        version: 1,
        commit_sha: promoted_sha.clone(),
        tests_ran: true,
        tests_passed: true,
        test_summary: None,
        captured_at: episode_entered_at + chrono::Duration::milliseconds(1),
        captured_by: "execution_complete".to_string(),
    };
    task.metadata = Some(
        cache
            .update_task_metadata(task.metadata.as_deref())
            .unwrap(),
    );
    state.task_repo.update(&task).await.unwrap();

    let mut step = TaskStep::new(task_id.clone(), "done".to_string(), 0, "test".to_string());
    step.status = TaskStepStatus::Completed;
    state.task_step_repo.create(step).await.unwrap();
    let mut conversation = ChatConversation::new_task(task_id.clone());
    conversation.context_type = ChatContextType::TaskExecution;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut agent_run = AgentRun::new(conversation.id);
    agent_run.status = AgentRunStatus::Completed;
    agent_run.completed_at = Some(chrono::Utc::now());
    state.agent_run_repo.create(agent_run).await.unwrap();

    let classification = classify_failed_restart(&state, &task).await;
    let FailedRestartClassification::RecoverToReview(evidence) = classification else {
        panic!("legacy validation cache should recover, got {classification:?}");
    };
    assert_eq!(evidence.validation_run_id, "legacy_validation_cache");
    assert_eq!(evidence.promoted_commit_sha, promoted_sha);
}

fn stopped_recovery_metadata(auto_recovery_count: u32) -> String {
    let mut recovery = ExecutionRecoveryMetadata::new();
    recovery.last_state = ExecutionRecoveryState::Failed;
    recovery.stop_retrying = true;
    recovery.auto_recovery_count = auto_recovery_count;
    recovery.unrecoverable_reason = Some(StopRetryingReason::ManualStop);
    recovery.append_event(ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::Failed,
        ExecutionRecoverySource::System,
        ExecutionRecoveryReasonCode::Unknown,
        "stopped before restart",
    ));
    recovery.update_task_metadata(None).unwrap()
}

#[tokio::test]
async fn failed_restart_step_cleanup_resets_only_failed_steps() {
    let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
    let task_id = TaskId::new();
    let mut failed = TaskStep::new(
        task_id.clone(),
        "Retry this step".to_string(),
        0,
        "test".to_string(),
    );
    failed.status = TaskStepStatus::Failed;
    failed.started_at = Some(chrono::Utc::now());
    failed.completed_at = Some(chrono::Utc::now());
    failed.completion_note = Some("failed".to_string());
    let failed = task_step_repo.create(failed).await.unwrap();
    let mut completed = TaskStep::new(
        task_id.clone(),
        "Keep this step".to_string(),
        1,
        "test".to_string(),
    );
    completed.status = TaskStepStatus::Completed;
    let completed = task_step_repo.create(completed).await.unwrap();

    let cleared = clear_failed_steps_for_failed_restart(&task_step_repo, &task_id)
        .await
        .expect("failed-step cleanup should succeed");

    assert_eq!(cleared, 1);
    let failed = task_step_repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(failed.status, TaskStepStatus::Pending);
    assert!(failed.started_at.is_none());
    assert!(failed.completed_at.is_none());
    assert!(failed.completion_note.is_none());
    assert_eq!(
        task_step_repo
            .get_by_id(&completed.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStepStatus::Completed
    );
}

#[tokio::test]
async fn failed_ready_restart_clears_stale_refs_and_preserves_completed_steps() {
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
    let project_id = ProjectId::from_string("project-restart".to_string());

    let mut task = Task::new(project_id, "Failed restart".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/stale".to_string());
    task.worktree_path = Some("/tmp/stale-worktree".to_string());
    task.merge_commit_sha = Some("deadbeef".to_string());
    task.metadata = Some(stopped_recovery_metadata(3));
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let mut completed_step = TaskStep::new(
        task_id.clone(),
        "completed".to_string(),
        0,
        "test".to_string(),
    );
    completed_step.status = TaskStepStatus::Completed;
    task_step_repo.create(completed_step).await.unwrap();

    let mut failed_step =
        TaskStep::new(task_id.clone(), "failed".to_string(), 1, "test".to_string());
    failed_step.status = TaskStepStatus::Failed;
    failed_step.started_at = Some(chrono::Utc::now());
    failed_step.completed_at = Some(chrono::Utc::now());
    failed_step.completion_note = Some("failed".to_string());
    task_step_repo.create(failed_step).await.unwrap();

    let preparation = prepare_terminal_task_for_ready_restart(&task_repo, &task_step_repo, &task)
        .await
        .unwrap();
    assert_eq!(preparation.cleared_failed_steps, 1);

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(updated_task.task_branch.is_none());
    assert!(updated_task.worktree_path.is_none());
    assert!(updated_task.merge_commit_sha.is_none());

    let metadata: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["trigger_origin"], "retry");
    assert_eq!(metadata["preserve_steps"], true);
    assert!(metadata.get("agent_variant").is_none());
    assert_eq!(
        metadata["execution_recovery"]["last_state"],
        serde_json::json!("retrying")
    );
    assert_eq!(
        metadata["execution_recovery"]["auto_recovery_count"],
        serde_json::json!(3)
    );

    let steps = task_step_repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(steps[0].status, TaskStepStatus::Completed);
    assert_eq!(steps[1].status, TaskStepStatus::Pending);
    assert!(steps[1].started_at.is_none());
    assert!(steps[1].completed_at.is_none());
    assert!(steps[1].completion_note.is_none());
}

#[tokio::test]
async fn cancelled_ready_restart_does_not_preserve_steps() {
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
    let project_id = ProjectId::from_string("project-cancelled-restart".to_string());

    let mut task = Task::new(project_id, "Cancelled restart".to_string());
    task.internal_status = InternalStatus::Cancelled;
    task.task_branch = Some("task/cancelled-stale".to_string());
    task.worktree_path = Some("/tmp/cancelled-stale-worktree".to_string());
    task.merge_commit_sha = Some("cafebabe".to_string());
    task.metadata = Some(stopped_recovery_metadata(2));
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let preparation = prepare_terminal_task_for_ready_restart(&task_repo, &task_step_repo, &task)
        .await
        .unwrap();

    assert_eq!(preparation.cleared_failed_steps, 0);

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(updated_task.task_branch.is_none());
    assert!(updated_task.worktree_path.is_none());
    assert!(updated_task.merge_commit_sha.is_none());

    let metadata: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["trigger_origin"], "retry");
    assert!(metadata.get("agent_variant").is_none());
    assert!(metadata.get("preserve_steps").is_none());
    assert_eq!(
        metadata["execution_recovery"]["last_state"],
        serde_json::json!("retrying")
    );
    assert_eq!(
        metadata["execution_recovery"]["auto_recovery_count"],
        serde_json::json!(2)
    );
    assert_eq!(
        metadata["execution_recovery"]["events"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        metadata["execution_recovery"]["stop_retrying"],
        serde_json::json!(false)
    );
    assert!(metadata["execution_recovery"]
        .get("unrecoverable_reason")
        .is_none());
}

#[tokio::test]
async fn stopped_ready_restart_clears_stale_refs_without_preserving_steps() {
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
    let project_id = ProjectId::from_string("project-stopped-restart".to_string());

    let mut task = Task::new(project_id, "Stopped restart".to_string());
    task.internal_status = InternalStatus::Stopped;
    task.task_branch = Some("task/stopped-stale".to_string());
    task.worktree_path = Some("/tmp/stopped-stale-worktree".to_string());
    task.merge_commit_sha = Some("0badcafe".to_string());
    task.metadata = Some(stopped_recovery_metadata(4));
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let preparation = prepare_terminal_task_for_ready_restart(&task_repo, &task_step_repo, &task)
        .await
        .unwrap();

    assert_eq!(preparation.cleared_failed_steps, 0);

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(updated_task.task_branch.is_none());
    assert!(updated_task.worktree_path.is_none());
    assert!(updated_task.merge_commit_sha.is_none());

    let metadata: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["trigger_origin"], "retry");
    assert!(metadata.get("preserve_steps").is_none());
    assert_eq!(
        metadata["execution_recovery"]["last_state"],
        serde_json::json!("retrying")
    );
    assert_eq!(
        metadata["execution_recovery"]["auto_recovery_count"],
        serde_json::json!(4)
    );
    assert_eq!(
        metadata["execution_recovery"]["stop_retrying"],
        serde_json::json!(false)
    );
}

#[tokio::test]
async fn terminal_ready_restart_rejects_stale_status_without_clearing_refs() {
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
    let project_id = ProjectId::from_string("project-stale-restart".to_string());

    let mut stale_task = Task::new(project_id, "Stale failed restart".to_string());
    stale_task.internal_status = InternalStatus::Failed;
    stale_task.task_branch = Some("task/preserve-on-race".to_string());
    stale_task.worktree_path = Some("/tmp/missing-stale-worktree".to_string());
    stale_task.merge_commit_sha = Some("preserve-on-race".to_string());

    let mut concurrent_task = stale_task.clone();
    concurrent_task.internal_status = InternalStatus::PendingReview;
    task_repo.create(concurrent_task.clone()).await.unwrap();

    let error = prepare_terminal_task_for_ready_restart(&task_repo, &task_step_repo, &stale_task)
        .await
        .expect_err("stale terminal preparation must lose optimistic authority");
    assert!(error.to_string().contains("changed concurrently"));

    let stored = task_repo.get_by_id(&stale_task.id).await.unwrap().unwrap();
    assert_eq!(stored.internal_status, InternalStatus::PendingReview);
    assert_eq!(stored.task_branch, concurrent_task.task_branch);
    assert_eq!(stored.worktree_path, concurrent_task.worktree_path);
    assert_eq!(stored.merge_commit_sha, concurrent_task.merge_commit_sha);
}

#[tokio::test]
async fn terminal_ready_restart_blocks_when_existing_worktree_is_dirty() {
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
    let project_id = ProjectId::from_string("project-dirty-restart".to_string());
    let worktree = tempfile::tempdir().expect("temp worktree");
    let git_init = std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg(worktree.path())
        .status()
        .expect("run git init");
    assert!(git_init.success(), "git init should succeed");
    write_test_file(&worktree.path().join("dirty.txt"), "dirty");
    let worktree_path = worktree.path().to_string_lossy().into_owned();

    let mut task = Task::new(project_id, "Dirty restart".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/dirty-stale".to_string());
    task.worktree_path = Some(worktree_path.clone());
    task.merge_commit_sha = Some("baadf00d".to_string());
    task.metadata = Some(stopped_recovery_metadata(5));
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let err = prepare_terminal_task_for_ready_restart(&task_repo, &task_step_repo, &task)
        .await
        .expect_err("dirty restart should be blocked");

    match err {
        AppError::Validation(message) => {
            assert!(message.contains("Cannot restart task"));
            assert!(message.contains("has uncommitted changes"));
        }
        other => panic!("expected validation error, got {other:?}"),
    }

    let stored = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(stored.task_branch.as_deref(), Some("task/dirty-stale"));
    assert_eq!(
        stored.worktree_path.as_deref(),
        Some(worktree_path.as_str())
    );
    assert_eq!(stored.merge_commit_sha.as_deref(), Some("baadf00d"));
}

#[tokio::test]
async fn non_terminal_ready_restart_preparation_is_noop() {
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
    let project_id = ProjectId::from_string("project-ready-noop".to_string());

    let mut task = Task::new(project_id, "Ready noop".to_string());
    task.internal_status = InternalStatus::Ready;
    task.task_branch = Some("task/keep".to_string());
    task.worktree_path = Some("/tmp/keep-worktree".to_string());
    task.merge_commit_sha = Some("feedface".to_string());

    let preparation = prepare_terminal_task_for_ready_restart(&task_repo, &task_step_repo, &task)
        .await
        .unwrap();

    assert_eq!(preparation, ReadyRestartPreparation::default());
    assert!(
        task_repo.get_by_id(&task.id).await.unwrap().is_none(),
        "non-terminal preparation should not persist a task update"
    );
}
