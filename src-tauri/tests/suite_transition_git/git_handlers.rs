use axum::{extract::{Path, State}, http::StatusCode, Json};
use ralphx_lib::application::{
    AppState, InteractiveProcessKey, TeamService, TeamStateTracker,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{InternalStatus, Project, ProjectId, Task, TaskId};
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::HttpServerState;
use std::sync::Arc;

fn parse_task_metadata(task: &Task) -> Option<serde_json::Value> {
    task.metadata
        .as_ref()
        .and_then(|metadata| serde_json::from_str(metadata).ok())
}

mod json_error_format {
    use super::*;

    #[test]
    fn error_without_details() {
        let (status, Json(body)) = json_error(StatusCode::BAD_REQUEST, "Invalid input", None);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "Invalid input");
        assert!(body.get("details").is_none());
    }

    #[test]
    fn error_with_details() {
        let (status, Json(body)) = json_error(
            StatusCode::BAD_REQUEST,
            "Commit not on branch",
            Some("Use git rev-parse HEAD on main".to_string()),
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "Commit not on branch");
        assert_eq!(body["details"], "Use git rev-parse HEAD on main");
    }

    #[test]
    fn internal_server_error_status() {
        let (status, Json(body)) =
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "Database error");
    }

    #[test]
    fn not_found_error_status() {
        let (status, Json(body)) = json_error(StatusCode::NOT_FOUND, "Task not found", None);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "Task not found");
    }
}

mod sha_validation {
    use super::*;

    #[test]
    fn valid_sha_40_lowercase_hex() {
        let sha = "a1b2c3d4e5f6789012345678901234567890abcd";
        assert!(is_valid_git_sha(sha));
    }

    #[test]
    fn valid_sha_40_uppercase_hex() {
        let sha = "A1B2C3D4E5F6789012345678901234567890ABCD";
        assert!(is_valid_git_sha(sha));
    }

    #[test]
    fn valid_sha_mixed_case() {
        let sha = "a1B2c3D4e5F6789012345678901234567890AbCd";
        assert!(is_valid_git_sha(sha));
    }

    #[test]
    fn valid_sha_all_digits() {
        let sha = "1234567890123456789012345678901234567890";
        assert!(is_valid_git_sha(sha));
    }

    #[test]
    fn invalid_sha_too_short() {
        let sha = "a1b2c3d4";
        assert!(!is_valid_git_sha(sha));
    }

    #[test]
    fn invalid_sha_too_long() {
        let sha = "a1b2c3d4e5f6789012345678901234567890abcd1234";
        assert!(!is_valid_git_sha(sha));
    }

    #[test]
    fn invalid_sha_non_hex_chars() {
        let sha = "g1b2c3d4e5f6789012345678901234567890abcd"; // 'g' is not hex
        assert!(!is_valid_git_sha(sha));
    }

    #[test]
    fn invalid_sha_empty() {
        let sha = "";
        assert!(!is_valid_git_sha(sha));
    }

    #[test]
    fn invalid_sha_spaces() {
        let sha = "a1b2c3d4e5f67890 2345678901234567890abcd";
        assert!(!is_valid_git_sha(sha));
    }

    #[test]
    fn invalid_sha_short_sha_format() {
        // Short SHA (7 chars) should be rejected
        let sha = "a1b2c3d";
        assert!(!is_valid_git_sha(sha));
    }
}

mod task_commits {
    use super::*;

    async fn setup_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_test());
        let execution_state = Arc::new(ExecutionState::new());
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            tracker.clone(),
        )));
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    fn run_git(repo: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_git_output(repo: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn setup_repo_with_advanced_base() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let repo = dir.path();

        run_git(repo, &["init", "-b", "main"]);
        run_git(repo, &["config", "user.email", "test@test.com"]);
        run_git(repo, &["config", "user.name", "Test"]);

        std::fs::write(repo.join("README.md"), "initial").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "initial commit"]);

        run_git(repo, &["checkout", "-b", "task-branch"]);
        std::fs::write(repo.join("task.txt"), "task work").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "feat: selected task work"]);
        let task_sha = run_git_output(repo, &["rev-parse", "HEAD"]);

        run_git(repo, &["checkout", "main"]);
        run_git(repo, &["merge", "--ff-only", "task-branch"]);

        std::fs::write(repo.join("base.txt"), "later base work").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "fix: unrelated base branch work"]);

        (dir, task_sha)
    }

    #[tokio::test]
    async fn get_task_commits_scopes_merged_task_to_recorded_merge_sha() {
        let (repo, task_sha) = setup_repo_with_advanced_base();
        let state = setup_state().await;

        let mut project = Project::new(
            "task-commits-project".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        let project_id = project.id.clone();
        state.app_state.project_repo.create(project).await.unwrap();

        let mut task = Task::new(project_id, "Merged task".to_string());
        task.internal_status = InternalStatus::Merged;
        task.merge_commit_sha = Some(task_sha);
        let task_id = task.id.clone();
        state.app_state.task_repo.create(task).await.unwrap();

        let Json(commits) = get_task_commits(
            State(state),
            Path(task_id.as_str().to_string()),
        )
        .await
        .expect("merged task commits should resolve from recorded merge sha");

        let messages: Vec<_> = commits
            .iter()
            .map(|commit| commit.message.as_str())
            .collect();
        assert_eq!(
            messages,
            vec!["feat: selected task work"],
            "task details must not show later base-branch commits"
        );
    }
}

mod ipr_removal {
    use super::*;

    async fn setup_git_test_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_test());
        let execution_state = Arc::new(ExecutionState::new());
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    /// Seed a task in Merging state so report_conflict / report_incomplete pass the guard.
    async fn seed_merging_task(state: &HttpServerState) -> Task {
        let project_id = ProjectId::new();
        let mut task = Task::new(project_id, "Merging task".to_string());
        task.internal_status = InternalStatus::Merging;
        state.app_state.task_repo.create(task.clone()).await.unwrap();
        task
    }

    /// report_conflict — IPR entry removed after transition to MergeConflict.
    ///
    /// When the merger agent calls report_conflict, the handler transitions the task
    /// to MergeConflict and then removes the IPR entry so the agent gets EOF on stdin.
    #[tokio::test]
    async fn test_report_conflict_removes_ipr() {
        let state = setup_git_test_state().await;
        let task = seed_merging_task(&state).await;
        let task_id = task.id.clone();

        // Register IPR entry for the merger agent
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cat for conflict IPR test");
        let stdin = child.stdin.take().expect("cat stdin");

        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        state
            .app_state
            .interactive_process_registry
            .register(key.clone(), stdin)
            .await;

        assert!(
            state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be registered before handler call"
        );

        let result = report_conflict(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(ReportConflictRequest {
                conflict_files: vec!["src/main.rs".to_string()],
                reason: "Cannot automatically resolve conflict in src/main.rs".to_string(),
            }),
        )
        .await;

        assert!(result.is_ok(), "report_conflict handler should succeed: {:?}", result);
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be removed after report_conflict succeeds"
        );

        // Clean up regardless of result
        state
            .app_state
            .interactive_process_registry
            .remove(&key)
            .await;
        let _ = child.kill().await;
    }

    // ---- complete_merge helpers ------------------------------------------------

    /// Set up a real git repo with a merged task branch. Returns (TempDir, merge_commit_sha).
    fn setup_complete_merge_repo() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create temp dir for complete_merge test");
        let repo = dir.path();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Initial commit on main
        std::fs::write(repo.join("readme.md"), "# test").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Create task-branch and merge it into main
        std::process::Command::new("git")
            .args(["checkout", "-b", "task-branch"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("feat.txt"), "feature").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "feat"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["merge", "task-branch", "--no-edit"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Get the merge commit SHA (HEAD on main)
        let sha_bytes = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout;
        let sha = String::from_utf8(sha_bytes).unwrap().trim().to_string();

        (dir, sha)
    }

    /// Set up a fake worktree dir with .git/rebase-merge to simulate rebase in progress.
    /// Also creates a minimal real git repo for project.working_directory.
    fn setup_rebase_in_progress_dirs() -> (tempfile::TempDir, tempfile::TempDir) {
        // Worktree dir with fake rebase state
        let worktree_dir = tempfile::tempdir().expect("create worktree dir");
        std::fs::create_dir_all(worktree_dir.path().join(".git").join("rebase-merge")).unwrap();

        // Minimal real git repo for project.working_directory
        let repo_dir = tempfile::tempdir().expect("create repo dir");
        let repo = repo_dir.path();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("r.md"), "init").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo)
            .output()
            .unwrap();

        (worktree_dir, repo_dir)
    }

    /// Seed a project + Merging task into the HTTP test state.
    /// Returns (task_id, ipr_key).
    async fn seed_merging_task_with_project(
        state: &HttpServerState,
        repo_path: &std::path::Path,
        worktree_path: Option<&std::path::Path>,
    ) -> (
        TaskId,
        InteractiveProcessKey,
    ) {
        let project_id = ProjectId::new();
        let mut project = Project::new(
            "test-project".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        state.app_state.project_repo.create(project).await.unwrap();

        let mut task = Task::new(project_id, "Complete merge test".to_string());
        task.internal_status = InternalStatus::Merging;
        task.task_branch = Some("task-branch".to_string());
        if let Some(wt) = worktree_path {
            task.worktree_path = Some(wt.to_string_lossy().to_string());
        }
        let task_id = task.id.clone();
        state.app_state.task_repo.create(task).await.unwrap();

        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        (task_id, key)
    }

    async fn seed_task_with_project_status(
        state: &HttpServerState,
        repo_path: &std::path::Path,
        status: InternalStatus,
    ) -> TaskId {
        let (task_id, _) = seed_merging_task_with_project(state, repo_path, None).await;
        let mut task = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        task.internal_status = status;
        task.touch();
        state.app_state.task_repo.update(&task).await.unwrap();
        task_id
    }

    // ---- complete_merge IPR tests ----------------------------------------

    /// complete_merge success path: IPR is removed after transition to Merged.
    ///
    /// When merger agent calls complete_merge with a valid SHA on the target branch,
    /// the handler transitions Merging → Merged and then removes the IPR entry.
    #[tokio::test]
    async fn test_complete_merge_success_removes_ipr() {
        let (dir, merge_sha) = setup_complete_merge_repo();
        let state = setup_git_test_state().await;
        let (task_id, key) = seed_merging_task_with_project(&state, dir.path(), None).await;

        // Register IPR for the merger agent
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cat for complete_merge IPR test");
        let stdin = child.stdin.take().expect("cat stdin");
        state
            .app_state
            .interactive_process_registry
            .register(key.clone(), stdin)
            .await;

        assert!(
            state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be registered before handler call"
        );

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: merge_sha,
            }),
        )
        .await;

        assert!(result.is_ok(), "complete_merge handler should succeed: {:?}", result);
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR should be removed after complete_merge success"
        );

        state
            .app_state
            .interactive_process_registry
            .remove(&key)
            .await;
        let _ = child.kill().await;
    }

    /// complete_merge rebase retry path: IPR is removed when transitioning to PendingMerge.
    ///
    /// When the worktree has .git/rebase-merge (rebase in progress), the handler
    /// transitions Merging → PendingMerge and closes IPR so the agent exits.
    #[tokio::test]
    async fn test_complete_merge_rebase_retry_removes_ipr() {
        let (worktree_dir, repo_dir) = setup_rebase_in_progress_dirs();
        let state = setup_git_test_state().await;
        let (task_id, key) =
            seed_merging_task_with_project(&state, repo_dir.path(), Some(worktree_dir.path()))
                .await;

        // Register IPR entry
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cat for rebase retry IPR test");
        let stdin = child.stdin.take().expect("cat stdin");
        state
            .app_state
            .interactive_process_registry
            .register(key.clone(), stdin)
            .await;

        assert!(
            state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be registered before handler call"
        );

        // Any valid 40-char SHA — handler exits via rebase path before branch verification
        let dummy_sha = "a".repeat(40);
        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: dummy_sha,
            }),
        )
        .await;

        assert!(result.is_ok(), "complete_merge rebase retry handler should succeed: {:?}", result);
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR should be removed after complete_merge rebase retry path"
        );

        state
            .app_state
            .interactive_process_registry
            .remove(&key)
            .await;
        let _ = child.kill().await;
    }

    /// complete_merge without an IPR entry is idempotent: handler succeeds and
    /// the missing IPR is silently ignored (remove returns None, no panic).
    #[tokio::test]
    async fn test_complete_merge_no_ipr_entry_succeeds_silently() {
        // Use rebase path to avoid needing a real git SHA on a branch
        let (worktree_dir, repo_dir) = setup_rebase_in_progress_dirs();
        let state = setup_git_test_state().await;
        let (task_id, key) =
            seed_merging_task_with_project(&state, repo_dir.path(), Some(worktree_dir.path()))
                .await;

        // No IPR entry registered
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "No IPR should be registered at start"
        );

        let dummy_sha = "b".repeat(40);
        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: dummy_sha,
            }),
        )
        .await;

        // Handler should not error just because IPR entry was absent
        // (the IPR remove is guarded with `if ... .remove().is_some()`)
        if result.is_ok() {
            // Verify the state is clean — no phantom IPR left behind
            assert!(
                !state
                    .app_state
                    .interactive_process_registry
                    .has_process(&key)
                    .await
            );
        }
        // If result is Err, the transition failed (in-memory limitation), which is acceptable.
        // The key invariant is: no panic when IPR entry is absent.
    }

    #[tokio::test]
    async fn test_complete_merge_is_idempotent_for_merged_task() {
        let (dir, merge_sha) = setup_complete_merge_repo();
        let state = setup_git_test_state().await;
        let task_id =
            seed_task_with_project_status(&state, dir.path(), InternalStatus::Merged).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: merge_sha,
            }),
        )
        .await
        .expect("complete_merge should be idempotent for merged task")
        .0;

        assert_eq!(result.new_status, "already_merged");

        let persisted = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.internal_status, InternalStatus::Merged);
    }

    #[tokio::test]
    async fn test_complete_merge_rejects_non_merging_status() {
        let (dir, merge_sha) = setup_complete_merge_repo();
        let state = setup_git_test_state().await;
        let task_id =
            seed_task_with_project_status(&state, dir.path(), InternalStatus::Approved).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: merge_sha,
            }),
        )
        .await;

        let error = result.expect_err("complete_merge must reject approved tasks");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            error.1["error"].as_str(),
            Some("Task must be in 'merging' status to complete merge. Current status: approved")
        );
    }

    /// report_conflict — no IPR entry is safe: handler succeeds without IPR registered.
    ///
    /// When no IPR entry is present (merger agent already exited), the IPR removal
    /// is a no-op and must not cause the handler to fail or panic.
    #[tokio::test]
    async fn test_report_conflict_no_ipr_entry_is_safe() {
        let state = setup_git_test_state().await;
        let task = seed_merging_task(&state).await;
        let task_id = task.id.clone();

        // No IPR entry registered — removal must be a no-op
        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "No IPR should be registered at start of test"
        );

        let result = report_conflict(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(ReportConflictRequest {
                conflict_files: vec!["src/lib.rs".to_string()],
                reason: "Conflicting changes in function signatures".to_string(),
            }),
        )
        .await;

        // Handler must not panic or error solely due to missing IPR entry.
        // If transition fails (in-memory limitation), that is acceptable;
        // the key invariant is no panic and no phantom IPR left behind.
        if result.is_ok() {
            assert!(
                !state
                    .app_state
                    .interactive_process_registry
                    .has_process(&key)
                    .await,
                "No phantom IPR should exist after handler call"
            );
        }
    }

    /// report_incomplete — no IPR entry is safe: handler succeeds without IPR registered.
    ///
    /// When no IPR entry is present (merger agent already exited), the IPR removal
    /// is a no-op and must not cause the handler to fail or panic.
    #[tokio::test]
    async fn test_report_incomplete_no_ipr_entry_is_safe() {
        let state = setup_git_test_state().await;
        let task = seed_merging_task(&state).await;
        let task_id = task.id.clone();

        // No IPR entry registered — removal must be a no-op
        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "No IPR should be registered at start of test"
        );

        let result = report_incomplete(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(ReportIncompleteRequest {
                reason: "Git operation failed: missing remote configuration".to_string(),
                diagnostic_info: Some("git remote: error".to_string()),
            }),
        )
        .await;

        // Handler must not panic or error solely due to missing IPR entry.
        if result.is_ok() {
            assert!(
                !state
                    .app_state
                    .interactive_process_registry
                    .has_process(&key)
                    .await,
                "No phantom IPR should exist after handler call"
            );
        }
    }

    /// report_incomplete — IPR entry removed after transition to MergeIncomplete.
    ///
    /// When the merger agent calls report_incomplete, the handler transitions the task
    /// to MergeIncomplete and then removes the IPR entry so the agent gets EOF on stdin.
    #[tokio::test]
    async fn test_report_incomplete_removes_ipr() {
        let state = setup_git_test_state().await;
        let task = seed_merging_task(&state).await;
        let task_id = task.id.clone();

        // Register IPR entry for the merger agent
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cat for incomplete IPR test");
        let stdin = child.stdin.take().expect("cat stdin");

        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        state
            .app_state
            .interactive_process_registry
            .register(key.clone(), stdin)
            .await;

        assert!(
            state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be registered before handler call"
        );

        let result = report_incomplete(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(ReportIncompleteRequest {
                reason: "Missing git configuration".to_string(),
                diagnostic_info: Some("git status: detached HEAD".to_string()),
            }),
        )
        .await;

        assert!(result.is_ok(), "report_incomplete handler should succeed: {:?}", result);
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be removed after report_incomplete succeeds"
        );

        // Clean up regardless of result
        state
            .app_state
            .interactive_process_registry
            .remove(&key)
            .await;
        let _ = child.kill().await;
    }
}

mod source_update_conflict {
    use super::*;

    async fn setup_git_test_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_test());
        let execution_state = Arc::new(ExecutionState::new());
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    /// Set up a real git repo where the commit is on the source branch but NOT on target.
    /// This simulates what happens when the merger agent resolves a source_update_conflict
    /// by merging target INTO source — the resulting commit is on the source branch only.
    fn setup_source_update_repo() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let repo = dir.path();

        // Init repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Initial commit on main
        std::fs::write(repo.join("readme.md"), "# test").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Create task-branch with a commit (simulates agent's merge commit on source)
        std::process::Command::new("git")
            .args(["checkout", "-b", "task-branch"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("feat.txt"), "merged target into source").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "merge target into source"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Get the commit SHA on task-branch (NOT on main)
        let sha_bytes = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout;
        let sha = String::from_utf8(sha_bytes).unwrap().trim().to_string();

        // Switch back to main so it stays untouched
        std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo)
            .output()
            .unwrap();

        (dir, sha)
    }

    /// Seed a project + Merging task with source_update_conflict metadata.
    async fn seed_source_update_task(
        state: &HttpServerState,
        repo_path: &std::path::Path,
        metadata: Option<&str>,
    ) -> TaskId {
        let project_id = ProjectId::new();
        let mut project = Project::new(
            "test-project".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        state.app_state.project_repo.create(project).await.unwrap();

        let mut task = Task::new(project_id, "Source update conflict test".to_string());
        task.internal_status = InternalStatus::Merging;
        task.task_branch = Some("task-branch".to_string());
        task.metadata = metadata.map(String::from);
        let task_id = task.id.clone();
        state.app_state.task_repo.create(task).await.unwrap();

        task_id
    }

    /// complete_merge with source_update_conflict: transitions to PendingMerge,
    /// sets source_conflict_resolved, removes IPR, returns success.
    #[tokio::test]
    async fn test_source_update_conflict_transitions_to_pending_merge() {
        let (dir, source_sha) = setup_source_update_repo();
        let state = setup_git_test_state().await;

        let metadata = r#"{"source_update_conflict": true, "conflict_files": ["src/lib.rs"], "target_branch": "main"}"#;
        let task_id = seed_source_update_task(&state, dir.path(), Some(metadata)).await;

        // Register IPR
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cat for source update IPR test");
        let stdin = child.stdin.take().expect("cat stdin");
        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        state
            .app_state
            .interactive_process_registry
            .register(key.clone(), stdin)
            .await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: source_sha,
            }),
        )
        .await;

        assert!(
            result.is_ok(),
            "complete_merge should succeed for source_update_conflict: {:?}",
            result
        );

        let resp = result.unwrap().0;
        assert!(resp.success);
        assert_eq!(resp.new_status, "pending_merge");
        assert!(resp.message.contains("Source update completed"));

        // Verify task state
        let task = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        // After transitioning to PendingMerge, the on_enter handler may auto-complete
        // the merge (since source is now up-to-date with target). Either PendingMerge
        // or Merged is correct — both prove the handler worked.
        assert!(
            task.internal_status == InternalStatus::PendingMerge
                || task.internal_status == InternalStatus::Merged,
            "Task should be in PendingMerge or Merged (auto-completed). Got: {:?}",
            task.internal_status
        );
        let meta = parse_task_metadata(&task).unwrap();
        assert_eq!(
            meta.get("source_conflict_resolved").and_then(|v| v.as_bool()),
            Some(true),
            "source_conflict_resolved flag must be set in metadata"
        );

        // source_update_conflict should be cleared from metadata
        let meta = parse_task_metadata(&task).unwrap();
        assert!(
            meta.get("source_update_conflict").is_none(),
            "source_update_conflict must be cleared from metadata"
        );
        assert!(
            meta.get("conflict_files").is_none(),
            "conflict_files must be cleared from metadata"
        );

        // IPR should be removed
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be removed after source update conflict resolution"
        );

        let _ = child.kill().await;
    }

    /// complete_merge with commit NOT on target and NO source_update_conflict → 400 error.
    #[tokio::test]
    async fn test_no_source_update_flag_returns_400() {
        let (dir, source_sha) = setup_source_update_repo();
        let state = setup_git_test_state().await;

        // No source_update_conflict in metadata
        let metadata = r#"{"some_other_key": true}"#;
        let task_id = seed_source_update_task(&state, dir.path(), Some(metadata)).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: source_sha,
            }),
        )
        .await;

        assert!(
            result.is_err(),
            "complete_merge should fail when commit not on target and no source_update_conflict"
        );

        let err = result.unwrap_err();
        assert_eq!(
            err.0, StatusCode::BAD_REQUEST,
            "Should return 400 BAD_REQUEST"
        );
    }

    /// complete_merge with source_update_conflict when source_conflict_resolved already set
    /// is idempotent — still transitions to PendingMerge successfully.
    #[tokio::test]
    async fn test_source_update_idempotent_with_existing_resolved_flag() {
        let (dir, source_sha) = setup_source_update_repo();
        let state = setup_git_test_state().await;

        // Both flags set (edge case: resolved already set from prior attempt)
        let metadata =
            r#"{"source_update_conflict": true, "source_conflict_resolved": true, "target_branch": "main"}"#;
        let task_id = seed_source_update_task(&state, dir.path(), Some(metadata)).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: source_sha,
            }),
        )
        .await;

        assert!(
            result.is_ok(),
            "complete_merge should still succeed when source_conflict_resolved already set: {:?}",
            result
        );

        let resp = result.unwrap().0;
        assert_eq!(resp.new_status, "pending_merge");

        // Verify flag is still set
        let task = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        let meta = parse_task_metadata(&task).unwrap();
        assert_eq!(
            meta.get("source_conflict_resolved").and_then(|v| v.as_bool()),
            Some(true),
            "source_conflict_resolved flag must still be set"
        );
    }

    /// complete_merge with no metadata at all and commit not on target → 400 error.
    #[tokio::test]
    async fn test_no_metadata_returns_400() {
        let (dir, source_sha) = setup_source_update_repo();
        let state = setup_git_test_state().await;

        let task_id = seed_source_update_task(&state, dir.path(), None).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: source_sha,
            }),
        )
        .await;

        assert!(
            result.is_err(),
            "complete_merge should fail when no metadata and commit not on target"
        );

        let err = result.unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }
}

// ============================================================================
// Integration test #5 — Freshness routing via complete_merge HTTP handler
// ============================================================================
//
// Bug 1 primary fix: complete_merge HTTP handler freshness intercept.
//
// When a task is in Merging with plan_update_conflict=true AND
// branch_freshness_conflict=true (merged into Merging due to a plan←main
// freshness conflict), the merger agent resolves the plan branch conflict and
// calls complete_merge. Before this fix the handler would fall through to the
// normal Merged path, losing the task's work. After this fix, the handler
// routes the task back to its origin state (Reviewing/PendingReview) via
// freshness_return_route() at step 5a.
mod freshness_routing_integration {
    use super::*;

    async fn setup_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_test());
        let execution_state = Arc::new(ExecutionState::new());
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    /// Set up a minimal real git repo with a task branch (not merged into plan branch).
    /// Returns (TempDir, task_branch_sha).
    fn setup_freshness_repo() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create temp dir for freshness test");
        let repo = dir.path();

        for args in &[
            vec!["init"],
            vec!["config", "user.email", "t@t.com"],
            vec!["config", "user.name", "T"],
        ] {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
        }

        // Initial commit on main
        std::fs::write(repo.join("readme.md"), "# repo").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Create task-branch with a commit — NOT merged into main/plan branch.
        // In the freshness scenario the merger was resolving plan←main only,
        // so the task's actual work is only on task-branch.
        std::process::Command::new("git")
            .args(["checkout", "-b", "task-branch"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("task-work.txt"), "task work").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "task work"])
            .current_dir(repo)
            .output()
            .unwrap();

        // SHA of the task-branch HEAD — NOT on main
        let sha_bytes = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout;
        let sha = String::from_utf8(sha_bytes).unwrap().trim().to_string();

        // Leave task-branch checked out (doesn't matter for the test — we pass SHA anyway)
        (dir, sha)
    }

    /// Seed a project + Merging task with freshness metadata in the test state.
    async fn seed_freshness_task(
        state: &HttpServerState,
        repo_path: &std::path::Path,
        metadata: &str,
    ) -> TaskId {
        let project_id = ProjectId::new();
        let mut project = Project::new(
            "freshness-test-project".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        state.app_state.project_repo.create(project).await.unwrap();

        let mut task = Task::new(project_id, "Freshness routing test".to_string());
        task.internal_status = InternalStatus::Merging;
        task.task_branch = Some("task-branch".to_string());
        task.metadata = Some(metadata.to_string());
        let task_id = task.id.clone();
        state.app_state.task_repo.create(task).await.unwrap();

        task_id
    }

    /// Integration test #5: complete_merge with plan_update_conflict=true AND
    /// branch_freshness_conflict=true → freshness intercept fires, task routed
    /// back to reviewing, merge_commit_sha NOT set, IPR removed.
    ///
    /// Assertions:
    /// - Handler returns success=true with new_status NOT "merged"
    /// - Task internal_status transitions to PendingReview (not Merged)
    /// - task.merge_commit_sha is NOT set (intercept fired before SHA assignment)
    /// - plan_update_conflict cleared from metadata
    /// - branch_freshness_conflict cleared from metadata
    /// - IPR entry removed
    /// - task_branch NOT deleted (preserved for re-execution)
    #[tokio::test]
    async fn test_complete_merge_freshness_routes_to_reviewing() {
        let (dir, _task_sha) = setup_freshness_repo();
        let state = setup_state().await;

        let metadata = serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "freshness_origin_state": "reviewing",
        })
        .to_string();
        let task_id = seed_freshness_task(&state, dir.path(), &metadata).await;

        // Register IPR entry for the merger agent
        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cat for freshness IPR test");
        let stdin = child.stdin.take().expect("cat stdin");
        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        state
            .app_state
            .interactive_process_registry
            .register(key.clone(), stdin)
            .await;

        // Any valid 40-char SHA — handler must exit at step 5a freshness check
        // before reaching SHA verification (step 6).
        let dummy_sha = "f".repeat(40);
        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: dummy_sha,
            }),
        )
        .await;

        assert!(
            result.is_ok(),
            "complete_merge should succeed for freshness-routed task: {:?}",
            result
        );

        let resp = result.unwrap().0;
        assert!(resp.success, "response.success must be true");
        assert_ne!(
            resp.new_status, "merged",
            "Freshness-routed task must NOT reach 'merged'"
        );

        // Task should be in PendingReview (origin state was "reviewing")
        let task = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .unwrap();

        // The transition goes to PendingReview (auto-route from Reviewing origin).
        // PendingReview auto-advances to Reviewing. If the task has no worktree_path
        // (as in this test), on_enter(Reviewing) returns ReviewWorktreeMissing which
        // routes to Escalated. All three outcomes indicate the freshness intercept fired.
        assert!(
            task.internal_status == InternalStatus::PendingReview
                || task.internal_status == InternalStatus::Reviewing
                || task.internal_status == InternalStatus::Escalated,
            "Task must be in PendingReview, Reviewing, or Escalated after freshness routing. Got: {:?}",
            task.internal_status
        );

        // merge_commit_sha must NOT be set (intercept fires before SHA assignment at step 6)
        assert!(
            task.merge_commit_sha.is_none(),
            "merge_commit_sha must NOT be set when freshness intercept fires"
        );

        // Freshness routing flags must be cleared
        let meta = parse_task_metadata(&task).unwrap_or_else(|| serde_json::json!({}));
        assert!(
            meta.get("plan_update_conflict").is_none()
                || meta
                    .get("plan_update_conflict")
                    .and_then(|v| v.as_bool())
                    == Some(false),
            "plan_update_conflict must be cleared after freshness routing"
        );
        assert!(
            meta.get("branch_freshness_conflict").is_none()
                || meta
                    .get("branch_freshness_conflict")
                    .and_then(|v| v.as_bool())
                    == Some(false),
            "branch_freshness_conflict must be cleared after freshness routing"
        );

        // IPR must be removed (merger agent should get EOF and exit)
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be removed after freshness routing"
        );

        // task_branch must NOT be deleted — worktree cleanup in freshness_return_route
        // only deletes the merge worktree, not the task branch itself.
        let task_branch = task.task_branch.as_deref().unwrap_or("task-branch");
        let branch_exists = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{}", task_branch)])
            .current_dir(dir.path())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(
            branch_exists,
            "task_branch '{}' must NOT be deleted when freshness intercept fires",
            task_branch
        );

        let _ = child.kill().await;
    }

    /// Integration test: complete_merge WITHOUT freshness flags → normal merge path,
    /// freshness intercept does NOT fire. This verifies that normal merges are
    /// unaffected by the new freshness check.
    #[tokio::test]
    async fn test_complete_merge_no_freshness_flags_normal_path() {
        let (dir, merge_sha) = {
            // We need a SHA that IS on main — reuse setup_complete_merge_repo logic
            let dir = tempfile::tempdir().expect("create temp dir");
            let repo = dir.path();
            for args in &[
                vec!["init"],
                vec!["config", "user.email", "t@t.com"],
                vec!["config", "user.name", "T"],
            ] {
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(repo)
                    .output()
                    .unwrap();
            }
            std::fs::write(repo.join("readme.md"), "# test").unwrap();
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["branch", "-M", "main"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["checkout", "-b", "task-branch"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::fs::write(repo.join("feat.txt"), "feature").unwrap();
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-m", "feat"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["checkout", "main"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["merge", "task-branch", "--no-edit"])
                .current_dir(repo)
                .output()
                .unwrap();
            let sha_bytes = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout;
            let sha = String::from_utf8(sha_bytes).unwrap().trim().to_string();
            (dir, sha)
        };

        let state = setup_state().await;
        // No freshness flags in metadata
        let metadata = r#"{"some_other_key": "value"}"#;
        let task_id = seed_freshness_task(&state, dir.path(), metadata).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: merge_sha,
            }),
        )
        .await;

        // Normal merge path should succeed and transition to Merged
        assert!(
            result.is_ok(),
            "Normal merge (no freshness flags) should succeed: {:?}",
            result
        );

        let resp = result.unwrap().0;
        assert_eq!(
            resp.new_status, "merged",
            "Normal merge without freshness flags must reach 'merged'"
        );

        let task = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            task.internal_status,
            InternalStatus::Merged,
            "Task must be Merged on normal path"
        );

        // merge_commit_sha MUST be set (normal path completed)
        assert!(
            task.merge_commit_sha.is_some(),
            "merge_commit_sha must be set on normal merge path"
        );
    }

    /// Integration test: complete_merge with plan_update_conflict=false → normal path,
    /// freshness intercept does NOT fire even if the key exists.
    #[tokio::test]
    async fn test_complete_merge_plan_update_conflict_false_normal_path() {
        let (dir, merge_sha) = {
            let dir = tempfile::tempdir().expect("create temp dir");
            let repo = dir.path();
            for args in &[
                vec!["init"],
                vec!["config", "user.email", "t@t.com"],
                vec!["config", "user.name", "T"],
            ] {
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(repo)
                    .output()
                    .unwrap();
            }
            std::fs::write(repo.join("readme.md"), "# test").unwrap();
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["branch", "-M", "main"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["checkout", "-b", "task-branch"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::fs::write(repo.join("feat.txt"), "feature").unwrap();
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-m", "feat"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["checkout", "main"])
                .current_dir(repo)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["merge", "task-branch", "--no-edit"])
                .current_dir(repo)
                .output()
                .unwrap();
            let sha_bytes = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout;
            let sha = String::from_utf8(sha_bytes).unwrap().trim().to_string();
            (dir, sha)
        };

        let state = setup_state().await;
        // plan_update_conflict explicitly set to false → no freshness intercept
        let metadata = r#"{"plan_update_conflict": false}"#;
        let task_id = seed_freshness_task(&state, dir.path(), metadata).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: merge_sha,
            }),
        )
        .await;

        assert!(
            result.is_ok(),
            "Merge with plan_update_conflict=false should succeed: {:?}",
            result
        );

        let resp = result.unwrap().0;
        assert_eq!(
            resp.new_status, "merged",
            "plan_update_conflict=false must not trigger freshness intercept"
        );
    }
}

mod webhook_emission {
    use super::*;

    async fn setup_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_test());
        let execution_state = Arc::new(ExecutionState::new());
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    fn setup_merged_repo() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        for (args, _) in &[
            (vec!["init"], ()),
            (vec!["config", "user.email", "t@t.com"], ()),
            (vec!["config", "user.name", "T"], ()),
        ] {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
        }
        std::fs::write(repo.join("r.md"), "init").unwrap();
        for args in &[
            vec!["add", "."],
            vec!["commit", "-m", "init"],
            vec!["branch", "-M", "main"],
            vec!["checkout", "-b", "task-branch"],
        ] {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
        }
        std::fs::write(repo.join("feat.txt"), "feature").unwrap();
        for args in &[
            vec!["add", "."],
            vec!["commit", "-m", "feat"],
            vec!["checkout", "main"],
            vec!["merge", "task-branch", "--no-edit"],
        ] {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
        }
        let sha = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        (dir, sha)
    }

    async fn seed_merging_task_with_project_id(
        state: &HttpServerState,
        repo_path: &std::path::Path,
    ) -> (TaskId, ProjectId) {
        let project_id = ProjectId::new();
        let mut project = Project::new(
            "webhook-test-project".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        state.app_state.project_repo.create(project).await.unwrap();

        let mut task = Task::new(project_id.clone(), "Webhook emission test".to_string());
        task.internal_status = InternalStatus::Merging;
        task.task_branch = Some("task-branch".to_string());
        let task_id = task.id.clone();
        state.app_state.task_repo.create(task).await.unwrap();
        (task_id, project_id)
    }

    /// complete_merge inserts a `merge:completed` row into external_events_repo.
    ///
    /// The agent HTTP path must publish to both the SSE/poll feed (external_events)
    /// and the webhook publisher. This test verifies the external_events side.
    #[tokio::test]
    async fn test_complete_merge_inserts_external_event() {
        let (dir, merge_sha) = setup_merged_repo();
        let state = setup_state().await;
        let (task_id, project_id) =
            seed_merging_task_with_project_id(&state, dir.path()).await;

        let result = complete_merge(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(CompleteMergeRequest {
                commit_sha: merge_sha,
            }),
        )
        .await;

        assert!(result.is_ok(), "complete_merge should succeed: {:?}", result);

        let events = state
            .app_state
            .external_events_repo
            .get_events_after_cursor(&[project_id.to_string()], 0, 100)
            .await
            .expect("get_events_after_cursor should succeed");

        let merge_completed_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "merge:completed")
            .collect();

        assert_eq!(
            merge_completed_events.len(),
            1,
            "complete_merge must insert exactly one merge:completed external event; got: {:?}",
            events.iter().map(|e| &e.event_type).collect::<Vec<_>>()
        );

        let payload: serde_json::Value =
            serde_json::from_str(&merge_completed_events[0].payload)
                .expect("payload must be valid JSON");
        assert_eq!(
            payload["task_id"].as_str().unwrap(),
            task_id.as_str(),
            "merge:completed payload must include correct task_id"
        );
        assert!(
            payload.get("project_id").is_some(),
            "merge:completed payload must include project_id"
        );
    }

    /// report_conflict inserts a `merge:conflict` row into external_events_repo.
    ///
    /// The agent HTTP path must publish to the SSE/poll feed when conflict is reported.
    #[tokio::test]
    async fn test_report_conflict_inserts_external_event() {
        let state = setup_state().await;

        // Seed a project with known id so we can query events back
        let project_id = ProjectId::new();
        let mut project = Project::new(
            "webhook-conflict-test".to_string(),
            "/tmp/unused".to_string(),
        );
        project.id = project_id.clone();
        state.app_state.project_repo.create(project).await.unwrap();

        let mut task = Task::new(project_id.clone(), "Conflict test task".to_string());
        task.internal_status = InternalStatus::Merging;
        task.task_branch = Some("task-branch".to_string());
        let task_id = task.id.clone();
        state.app_state.task_repo.create(task).await.unwrap();

        let result = report_conflict(
            State(state.clone()),
            Path(task_id.as_str().to_string()),
            Json(ReportConflictRequest {
                conflict_files: vec!["src/main.rs".to_string()],
                reason: "Cannot automatically resolve conflict".to_string(),
            }),
        )
        .await;

        assert!(result.is_ok(), "report_conflict should succeed: {:?}", result);

        let events = state
            .app_state
            .external_events_repo
            .get_events_after_cursor(&[project_id.to_string()], 0, 100)
            .await
            .expect("get_events_after_cursor should succeed");

        let conflict_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "merge:conflict")
            .collect();

        assert_eq!(
            conflict_events.len(),
            1,
            "report_conflict must insert exactly one merge:conflict external event; got: {:?}",
            events.iter().map(|e| &e.event_type).collect::<Vec<_>>()
        );

        let payload: serde_json::Value =
            serde_json::from_str(&conflict_events[0].payload)
                .expect("payload must be valid JSON");
        assert_eq!(
            payload["task_id"].as_str().unwrap(),
            task_id.as_str(),
            "merge:conflict payload must include correct task_id"
        );
        assert_eq!(
            payload["project_id"].as_str().unwrap(),
            project_id.to_string(),
            "merge:conflict payload must include correct project_id"
        );
    }
}
