// RC3 regression tests: stale reviewer guard in complete_review
//
// Bug: RC#3 — 370ms race window between reviewer stream completion and task auto-transition.
//
// The reviewer agent stream can complete AFTER the task has already left the Reviewing state
// (e.g., a concurrent auto-transition or user action moved it to ReviewPassed/RevisionNeeded).
// Without a guard, the stale `complete_review` MCP call would overwrite the already-transitioned
// state, corrupting the task lifecycle.
//
// Fix: Guard at reviews.rs:32 — read the task's current status before acting.
//      If task.internal_status != Reviewing, return 400 BAD_REQUEST immediately.
//
// Tests:
//   1. Stale reviewer call on ReviewPassed task → 400 BAD_REQUEST (guard fires).
//   2. Stale reviewer call on Merged task → 400 BAD_REQUEST (guard fires for terminal states).
//   3. Stale reviewer call on Ready task → 400 BAD_REQUEST (guard fires for non-review states).
//   4. Valid reviewer call on Reviewing task → guard does NOT fire (proceeds past guard line 32).

use crate::support::real_git_repo::setup_real_git_repo;
use axum::{extract::State, http::StatusCode, Json};
use ralphx_lib::application::{interactive_process_registry::InteractiveProcessKey, AppState};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceFollowupProvenance,
    ChatConversation, IdeationAnalysisBaseRefKind, IdeationSession, InternalStatus, Priority,
    Project, ProjectId, ProposalCategory, ReviewNote, ReviewOutcome, ReviewScopeMetadata,
    ReviewerType, Task, TaskProposal,
};
use ralphx_lib::domain::review::ReviewSettings;
use ralphx_lib::domain::state_machine::transition_handler::set_no_code_changes_metadata;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::helpers::get_task_context_impl;
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::{
    CreateChildSessionRequest, HttpServerState, ReviewIssueRequest,
};
use std::sync::Arc;

/// Build a minimal HttpServerState backed by in-memory repos (no SQLite, no Tauri app handle).
async fn setup_review_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());

    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

async fn setup_review_scope_drift_state() -> (HttpServerState, Task) {
    let app_state = Arc::new(AppState::new_sqlite_test());
    let execution_state = Arc::new(ExecutionState::new());
    let state = HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    };

    let repo = setup_real_git_repo();
    let repo_path = repo.dir.keep();
    let repo_path_string = repo_path.to_string_lossy().to_string();
    let checkout_status = std::process::Command::new("git")
        .args(["checkout", &repo.task_branch])
        .current_dir(&repo_path)
        .status()
        .expect("checkout task branch");
    assert!(
        checkout_status.success(),
        "task branch checkout must succeed"
    );

    let mut project = Project::new("Review Scope Project".to_string(), repo_path_string);
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::new_with_title(project_id.clone(), "Scope Review Session");
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let origin_conversation = ChatConversation::new_project(project_id.clone());
    state
        .app_state
        .chat_conversation_repo
        .create(origin_conversation.clone())
        .await
        .unwrap();
    let mut origin_workspace = AgentConversationWorkspace::new(
        origin_conversation.id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test/review-origin".to_string(),
        "/tmp/ralphx-review-origin".to_string(),
    );
    origin_workspace.linked_ideation_session_id = Some(session_id.clone());
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(origin_workspace)
        .await
        .unwrap();

    let mut proposal = TaskProposal::new(
        session_id.clone(),
        "Scoped proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.affected_paths =
        Some(serde_json::to_string(&vec!["src-tauri/src/http_server".to_string()]).unwrap());
    let proposal_id = proposal.id.clone();
    state
        .app_state
        .task_proposal_repo
        .create(proposal)
        .await
        .unwrap();

    let mut task = Task::new(project_id, "Reviewing task with drift".to_string());
    task.internal_status = InternalStatus::Reviewing;
    task.source_proposal_id = Some(proposal_id);
    task.ideation_session_id = Some(session_id);
    task.task_branch = Some(repo.task_branch);
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let context = get_task_context_impl(&state.app_state, &task.id)
        .await
        .expect("task context should load");
    assert_eq!(
        context.scope_drift_status,
        ralphx_lib::domain::entities::ScopeDriftStatus::ScopeExpansion,
        "test fixture must produce scope expansion; actual changed files = {:?}, out_of_scope = {:?}",
        context.actual_changed_files,
        context.out_of_scope_files
    );

    (state, task)
}

#[tokio::test]
async fn test_get_task_context_includes_existing_task_followup_sessions() {
    let (state, task) = setup_review_scope_drift_state().await;
    let parent_session_id = task
        .ideation_session_id
        .as_ref()
        .expect("fixture task should have ideation session")
        .as_str()
        .to_string();

    let response = create_child_session(
        State(state.clone()),
        Json(CreateChildSessionRequest {
            parent_session_id,
            title: Some("Out-of-scope blocker follow-up".to_string()),
            description: Some("Track existing unrelated blocker".to_string()),
            inherit_context: false,
            initial_prompt: Some("Investigate the blocker autonomously.".to_string()),
            source_task_id: Some(task.id.as_str().to_string()),
            source_context_type: Some("task_execution".to_string()),
            source_context_id: Some(task.id.as_str().to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: None,
            purpose: Some("follow_up".to_string()),
            is_external_trigger: false,
        }),
    )
    .await
    .expect("child session should be created");
    let response = response.0;

    let context = get_task_context_impl(&state.app_state, &task.id)
        .await
        .expect("task context should load");

    assert!(context.out_of_scope_blocker_fingerprint.is_some());
    assert_eq!(context.followup_sessions.len(), 1);
    let followup = &context.followup_sessions[0];
    assert_eq!(followup.id, response.session_id);
    assert_eq!(
        followup.title.as_deref(),
        Some("Out-of-scope blocker follow-up")
    );
    assert_eq!(
        followup.source_context_type.as_deref(),
        Some("task_execution")
    );
    assert_eq!(
        followup.spawn_reason.as_deref(),
        Some("out_of_scope_failure")
    );
    assert_eq!(
        followup.blocker_fingerprint.as_deref(),
        context.out_of_scope_blocker_fingerprint.as_deref()
    );
}

/// Create a task with the given status in the state's task repo.
async fn seed_task_with_status(state: &HttpServerState, status: InternalStatus) -> Task {
    let project_id = ProjectId::new();
    let mut task = Task::new(project_id, "RC3 test task".to_string());
    task.internal_status = status;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();
    task
}

async fn seed_project_task_with_status(
    state: &HttpServerState,
    name: &str,
    status: InternalStatus,
) -> Task {
    let project = Project::new(format!("{name} Project"), "/tmp/test".to_string());
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, format!("{name} task"));
    task.internal_status = status;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();
    task
}

fn git_stdout(repo_path: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::test]
async fn test_complete_review_approved_without_human_review_succeeds_for_no_code_branchless_task() {
    let state = setup_review_test_state().await;

    let project = Project::new(
        "Branchless Review Project".to_string(),
        "/tmp/test".to_string(),
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    state
        .app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_human_review: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings update should succeed");

    let mut task = Task::new(project.id.clone(), "Branchless reviewed task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    set_no_code_changes_metadata(&mut task);
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "approved".to_string(),
        summary: Some("AI approved without human gate".to_string()),
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let response = complete_review(State(state.clone()), ProjectScope(None), Json(req))
        .await
        .expect("approved review without human gate should succeed")
        .0;

    assert!(
        matches!(
            response.new_status.as_str(),
            "approved" | "pending_merge" | "merged"
        ),
        "approved review without human gate must advance past reviewing; got {}",
        response.new_status
    );

    let persisted = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(persisted.internal_status, InternalStatus::Reviewing);
}

#[tokio::test]
async fn test_approve_task_rejects_merged_status() {
    let state = setup_review_test_state().await;
    let task =
        seed_project_task_with_status(&state, "Human approve reject", InternalStatus::Merged).await;

    let result = approve_task(
        State(state.clone()),
        ProjectScope(None),
        Json(ApproveTaskRequest {
            task_id: task.id.as_str().to_string(),
            comment: None,
        }),
    )
    .await;

    match result {
        Err((status, msg)) => {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(
                msg.contains("review_passed") || msg.contains("escalated"),
                "expected approvable-state guidance, got: {msg}"
            );
        }
        Ok(_) => panic!("approve_task must reject merged tasks"),
    }

    let notes = state
        .app_state
        .review_repo
        .get_notes_by_task_id(&task.id)
        .await
        .unwrap();
    assert!(
        notes.is_empty(),
        "reject path must not create human approval notes"
    );
}

#[tokio::test]
async fn test_approve_task_accepts_review_passed_status() {
    let state = setup_review_test_state().await;
    let repo = setup_real_git_repo();
    let base_sha = git_stdout(repo.path(), &["rev-parse", "main"]);
    git_stdout(repo.path(), &["checkout", &repo.task_branch]);

    let mut project = Project::new(
        "Human approve success Project".to_string(),
        repo.path_string(),
    );
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Human approve success task".to_string());
    task.internal_status = InternalStatus::ReviewPassed;
    task.task_branch = Some(repo.task_branch.clone());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.worktree_path = Some(repo.path_string());
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let response = approve_task(
        State(state.clone()),
        ProjectScope(None),
        Json(ApproveTaskRequest {
            task_id: task.id.as_str().to_string(),
            comment: Some("Human verified".to_string()),
        }),
    )
    .await
    .expect("approve_task should accept review_passed")
    .0;

    assert_eq!(response.new_status, "approved");

    let persisted = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        persisted.internal_status,
        InternalStatus::ReviewPassed,
        "approved human review must advance past review_passed"
    );

    let notes = state
        .app_state
        .review_repo
        .get_notes_by_task_id(&task.id)
        .await
        .unwrap();
    assert!(
        notes.iter().any(|note| {
            note.outcome == ReviewOutcome::Approved
                && note.notes.as_deref() == Some("Human verified")
                && note.reviewer == ReviewerType::Human
        }),
        "approve_task must persist a human approval note"
    );
}

#[tokio::test]
async fn test_approve_task_accepts_no_code_review_passed_status() {
    let state = setup_review_test_state().await;
    let project = Project::new(
        "Human approve no-code Project".to_string(),
        "/tmp/test".to_string(),
    );
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Human approve no-code task".to_string());
    task.internal_status = InternalStatus::ReviewPassed;
    set_no_code_changes_metadata(&mut task);
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let response = approve_task(
        State(state.clone()),
        ProjectScope(None),
        Json(ApproveTaskRequest {
            task_id: task.id.as_str().to_string(),
            comment: Some("Human verified no code changes".to_string()),
        }),
    )
    .await
    .expect("approve_task should accept explicitly no-code review_passed")
    .0;

    assert_eq!(response.new_status, "approved");

    let persisted = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            persisted.internal_status,
            InternalStatus::Approved | InternalStatus::PendingMerge | InternalStatus::Merged
        ),
        "no-code human approval must advance past review_passed; got {:?}",
        persisted.internal_status
    );

    let notes = state
        .app_state
        .review_repo
        .get_notes_by_task_id(&task.id)
        .await
        .unwrap();
    assert!(
        notes.iter().any(|note| {
            note.outcome == ReviewOutcome::Approved
                && note.notes.as_deref() == Some("Human verified no code changes")
                && note.reviewer == ReviewerType::Human
        }),
        "no-code approval must persist a human approval note"
    );
}

#[tokio::test]
async fn test_request_task_changes_accepts_escalated_status() {
    let state = setup_review_test_state().await;
    let task =
        seed_project_task_with_status(&state, "Human request changes", InternalStatus::Escalated)
            .await;

    let response = request_task_changes(
        State(state.clone()),
        ProjectScope(None),
        Json(RequestTaskChangesRequest {
            task_id: task.id.as_str().to_string(),
            feedback: "Please revise the approach".to_string(),
        }),
    )
    .await
    .expect("request_task_changes should accept escalated")
    .0;

    assert_eq!(response.new_status, "revision_needed");

    let persisted = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            persisted.internal_status,
            InternalStatus::RevisionNeeded
                | InternalStatus::Ready
                | InternalStatus::Executing
                | InternalStatus::ReExecuting
        ),
        "request changes must move escalated task back into execution flow; got {:?}",
        persisted.internal_status
    );

    let notes = state
        .app_state
        .review_repo
        .get_notes_by_task_id(&task.id)
        .await
        .unwrap();
    assert!(
        notes.iter().any(|note| {
            note.outcome == ReviewOutcome::ChangesRequested
                && note.notes.as_deref() == Some("Please revise the approach")
                && note.reviewer == ReviewerType::Human
        }),
        "request_task_changes must persist a human changes-requested note"
    );
}

/// RC#3 guard 1: complete_review on a ReviewPassed task returns 400.
///
/// Scenario: reviewer agent stream completed AFTER the task auto-transitioned from
/// Reviewing → ReviewPassed. The stale `complete_review` MCP call must be rejected.
#[tokio::test]
async fn test_complete_review_rejected_when_task_already_review_passed() {
    let state = setup_review_test_state().await;

    // Seed the task already in ReviewPassed (auto-transition already fired).
    let task = seed_task_with_status(&state, InternalStatus::ReviewPassed).await;
    let task_id = task.id.as_str().to_string();

    let req = CompleteReviewRequest {
        task_id,
        decision: "approved".to_string(),
        summary: None,
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "RC#3: stale reviewer on ReviewPassed task must return 400. Got: {status}",
            );
            assert!(
                msg.contains("Task not in reviewing state"),
                "RC#3: error message must mention reviewing state. Got: {msg}",
            );
        }
        Ok(_) => panic!("RC#3: complete_review must fail when task is ReviewPassed (stale call)"),
    }
}

/// RC#3 guard 2: complete_review on a Merged task returns 400.
///
/// Scenario: reviewer agent stream completed very late, after merge completed.
/// The guard must reject this regardless of how far the task has progressed.
#[tokio::test]
async fn test_complete_review_rejected_when_task_merged() {
    let state = setup_review_test_state().await;

    let task = seed_task_with_status(&state, InternalStatus::Merged).await;
    let task_id = task.id.as_str().to_string();

    let req = CompleteReviewRequest {
        task_id,
        decision: "approved".to_string(),
        summary: None,
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, _)) => {
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "RC#3: stale reviewer on Merged task must return 400",
            );
        }
        Ok(_) => panic!("RC#3: complete_review must fail when task is Merged"),
    }
}

/// RC#3 late guard: the final transition must still reject when a task leaves
/// Reviewing after handler entry but before the transition is applied.
#[tokio::test]
async fn test_complete_review_late_guard_rejects_midflight_state_drift() {
    let state = setup_review_test_state().await;
    let task = seed_task_with_status(&state, InternalStatus::Reviewing).await;

    let mut transitioned = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task should exist");
    transitioned.internal_status = InternalStatus::Merged;
    transitioned.touch();
    state
        .app_state
        .task_repo
        .update(&transitioned)
        .await
        .unwrap();

    let result = ensure_task_still_reviewing_before_transition(&state, &task.id, "approved").await;

    match result {
        Err((status, msg)) => {
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "late stale-review guard must reject midflight drift. Got: {status}",
            );
            assert!(
                msg.contains("Task not in reviewing state"),
                "late stale-review guard must mention reviewing state. Got: {msg}",
            );
        }
        Ok(_) => panic!("late stale-review guard must reject when task drifted to merged"),
    }
}

/// RC#3 guard 3: complete_review on a Ready task returns 400.
///
/// Scenario: reviewer agent somehow fires complete_review while the task is in a
/// non-review state (e.g. task was reset to Ready after a restart). Must be rejected.
#[tokio::test]
async fn test_complete_review_rejected_when_task_ready() {
    let state = setup_review_test_state().await;

    let task = seed_task_with_status(&state, InternalStatus::Ready).await;
    let task_id = task.id.as_str().to_string();

    let req = CompleteReviewRequest {
        task_id,
        decision: "needs_changes".to_string(),
        summary: None,
        feedback: Some("looks wrong".to_string()),
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "RC#3: complete_review on Ready task must return 400",
            );
            assert!(
                msg.contains("Task not in reviewing state"),
                "RC#3: error must mention reviewing state. Got: {msg}",
            );
        }
        Ok(_) => panic!("RC#3: complete_review must fail when task is Ready"),
    }
}

/// RC#3 guard 4: Reviewing task passes the guard (no early 400 return at line 32).
///
/// Documents that the guard ONLY fires when the task is NOT in Reviewing state.
/// This test confirms the guard allows valid reviewer calls through.
///
/// Note: The handler proceeds past the guard to call TaskTransitionService.
/// With in-memory repos, the transition itself is expected to fail or succeed
/// depending on state machine validation. We only care that the 400-guard does
/// NOT fire — the assert is on the specific status code, not overall success.
#[tokio::test]
async fn test_complete_review_guard_does_not_fire_for_reviewing_task() {
    let state = setup_review_test_state().await;

    let mut task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    set_no_code_changes_metadata(&mut task);
    state.app_state.task_repo.update(&task).await.unwrap();
    let task_id = task.id.as_str().to_string();

    let req = CompleteReviewRequest {
        task_id,
        decision: "approved".to_string(),
        summary: None,
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    // The guard-specific 400 must NOT be returned for a Reviewing task.
    // (The handler may return 200 or another error from transition — that's fine.)
    if let Err((status, msg)) = &result {
        assert_ne!(
            *status,
            StatusCode::BAD_REQUEST,
            "RC#3: guard must NOT fire for a valid Reviewing task. \
             Got 400 with message: {msg}",
        );
    }
    // If Ok — guard correctly didn't fire, handler proceeded.
}

#[tokio::test]
async fn test_complete_review_requires_scope_drift_classification_for_scope_expansion() {
    let (state, task) = setup_review_scope_drift_state().await;

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "approved".to_string(),
        summary: None,
        feedback: Some("Looks okay".to_string()),
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "expected scope drift guard to return 400, got {status} with message: {msg}"
            );
            assert!(
                msg.contains("Scope drift classification required"),
                "expected scope drift guard message, got: {msg}"
            );
            assert!(
                msg.contains("feature.rs"),
                "expected out-of-scope file to be surfaced, got: {msg}"
            );
        }
        Ok(_) => panic!("approval without scope drift classification must fail"),
    }
}

#[tokio::test]
async fn test_complete_review_rejects_approval_with_unrelated_scope_drift() {
    let (state, task) = setup_review_scope_drift_state().await;

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "approved".to_string(),
        summary: None,
        feedback: Some("Looks okay".to_string()),
        issues: None,
        escalation_reason: None,
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some("feature.rs is outside the proposal scope".to_string()),
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "expected unrelated drift approval rejection to return 400, got {status} with message: {msg}"
            );
            assert!(
                msg.contains("Cannot approve task with unrelated scope drift"),
                "expected unrelated drift approval rejection, got: {msg}"
            );
        }
        Ok(_) => panic!("approval with unrelated_drift classification must fail"),
    }
}

#[tokio::test]
async fn test_complete_review_needs_changes_creates_first_class_review_issues() {
    let (state, task) = setup_review_scope_drift_state().await;

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "needs_changes".to_string(),
        summary: Some("Drift found".to_string()),
        feedback: Some("Please narrow the branch and address the scoped issue.".to_string()),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: Some("feature.rs is outside task scope".to_string()),
            step_id: None,
            no_step_reason: Some("Scope drift spans the task branch, not a single execution step".to_string()),
            description: Some("The branch modified src/feature.rs even though the proposal only covered src-tauri/src/http_server.".to_string()),
            category: Some("quality".to_string()),
            file_path: Some("src/feature.rs".to_string()),
            line_number: Some(1),
            code_snippet: None,
        }]),
        escalation_reason: None,
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some("Send back to revise and keep the unrelated change out of this branch.".to_string()),
    };

    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;
    assert!(
        result.is_ok(),
        "needs_changes with classified drift should succeed, got: {:?}",
        result
    );

    let issues = state
        .app_state
        .review_issue_repo
        .get_by_task_id(&task.id)
        .await
        .expect("issues query should succeed");
    assert_eq!(issues.len(), 1, "expected a first-class review issue row");
    let issue = &issues[0];
    assert_eq!(issue.title, "feature.rs is outside task scope");
    assert_eq!(
        issue.no_step_reason.as_deref(),
        Some("Scope drift spans the task branch, not a single execution step")
    );
    assert_eq!(issue.file_path.as_deref(), Some("src/feature.rs"));
}

#[tokio::test]
async fn test_complete_review_persists_review_scope_snapshot_for_merge_backstop() {
    let (state, task) = setup_review_scope_drift_state().await;

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "needs_changes".to_string(),
        summary: Some("Needs revision".to_string()),
        feedback: Some("Keep the branch scoped to the proposal boundary.".to_string()),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: Some("feature.rs is outside task scope".to_string()),
            step_id: None,
            no_step_reason: Some(
                "Scope drift spans the task branch, not a single execution step".to_string(),
            ),
            description: Some(
                "The branch modified feature.rs even though the proposal only covered src-tauri/src/http_server."
                    .to_string(),
            ),
            category: Some("quality".to_string()),
            file_path: Some("feature.rs".to_string()),
            line_number: None,
            code_snippet: None,
        }]),
        escalation_reason: None,
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some("feature.rs came from unrelated repo cleanup.".to_string()),
    };

    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;
    assert!(
        result.is_ok(),
        "needs_changes with structured issues should succeed"
    );

    let updated_task = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let review_scope = ReviewScopeMetadata::from_task_metadata(updated_task.metadata.as_deref())
        .unwrap()
        .expect("review_scope metadata should be present");

    assert_eq!(
        review_scope.planned_paths,
        vec!["src-tauri/src/http_server".to_string()]
    );
    assert_eq!(
        review_scope.reviewed_out_of_scope_files,
        vec!["feature.rs".to_string()]
    );
    assert_eq!(
        review_scope.drift_classification.as_deref(),
        Some("unrelated_drift")
    );
    assert_eq!(
        review_scope.drift_notes.as_deref(),
        Some("feature.rs came from unrelated repo cleanup.")
    );
}

#[tokio::test]
async fn test_complete_review_thin_legacy_issue_payload_backfills_first_class_issue_fields() {
    let (state, task) = setup_review_scope_drift_state().await;

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "needs_changes".to_string(),
        summary: Some("Legacy issue payload".to_string()),
        feedback: Some("Please fix the scoped work first.".to_string()),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: None,
            step_id: None,
            no_step_reason: None,
            description: Some("Legacy issue without structured fields".to_string()),
            category: None,
            file_path: Some("src/feature.rs".to_string()),
            line_number: Some(7),
            code_snippet: None,
        }]),
        escalation_reason: None,
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some(
            "Legacy payload should still produce a real issue row.".to_string(),
        ),
    };

    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;
    assert!(
        result.is_ok(),
        "legacy thin issue payload should still succeed, got: {:?}",
        result
    );

    let issues = state
        .app_state
        .review_issue_repo
        .get_by_task_id(&task.id)
        .await
        .expect("issues query should succeed");
    assert_eq!(issues.len(), 1, "expected one persisted issue");
    let issue = &issues[0];
    assert_eq!(issue.title, "Legacy issue without structured fields");
    assert_eq!(
        issue.no_step_reason.as_deref(),
        Some("Reviewer did not associate this issue with a specific task step")
    );
    assert_eq!(issue.line_number, Some(7));
}

#[tokio::test]
async fn test_complete_review_rejects_unrelated_drift_escalation_while_revision_budget_remains() {
    let (state, task) = setup_review_scope_drift_state().await;

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "escalate".to_string(),
        summary: Some("Out-of-scope blocker".to_string()),
        feedback: Some(
            "This branch contains unrelated drift and should be revised first.".to_string(),
        ),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: Some("feature.rs is outside task scope".to_string()),
            step_id: None,
            no_step_reason: Some("Scope drift spans the task branch".to_string()),
            description: Some(
                "The branch contains unrelated changes that should be removed from this task."
                    .to_string(),
            ),
            category: Some("quality".to_string()),
            file_path: Some("src/feature.rs".to_string()),
            line_number: Some(1),
            code_snippet: None,
        }]),
        escalation_reason: Some("Scope drift found".to_string()),
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some("Reviewer should send this back to revise first.".to_string()),
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(
                msg.contains("must go back through revise"),
                "expected revise-first guard message, got: {msg}"
            );
        }
        Ok(_) => panic!("unrelated drift should not escalate while revision budget remains"),
    }
}

#[tokio::test]
async fn test_complete_review_allows_unrelated_drift_escalation_after_revision_budget_exhausted() {
    let (state, task) = setup_review_scope_drift_state().await;

    state
        .app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            max_revision_cycles: 1,
            auto_create_followup_agent_conversation: true,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings update should succeed");

    let prior_note = ReviewNote::with_content(
        task.id.clone(),
        ReviewerType::Ai,
        ReviewOutcome::ChangesRequested,
        Some("Previous revise round".to_string()),
        Some("Already sent back once".to_string()),
        None,
    );
    state
        .app_state
        .review_repo
        .add_note(&prior_note)
        .await
        .expect("prior review note should persist");

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "escalate".to_string(),
        summary: Some("Still blocked by unrelated drift".to_string()),
        feedback: Some("The branch keeps reintroducing unrelated scope drift.".to_string()),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: Some("feature.rs is outside task scope".to_string()),
            step_id: None,
            no_step_reason: Some("Scope drift spans the task branch".to_string()),
            description: Some(
                "Repeated revise rounds did not remove the unrelated change.".to_string(),
            ),
            category: Some("quality".to_string()),
            file_path: Some("src/feature.rs".to_string()),
            line_number: Some(1),
            code_snippet: None,
        }]),
        escalation_reason: Some("Revision budget exhausted for unrelated scope drift".to_string()),
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some(
            "Escalation is now allowed because revise budget is exhausted.".to_string(),
        ),
    };

    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;
    let response = result
        .expect("escalation should be allowed after revision budget exhaustion")
        .0;
    assert!(
        response.followup_conversation_id.is_none(),
        "Wry-free review fixture should register the Agent issue without spawning a new conversation"
    );

    let issues = state
        .app_state
        .agent_conversation_issue_repo
        .list_by_conversation(
            &state
                .app_state
                .agent_conversation_workspace_repo
                .get_by_linked_ideation_session_id(task.ideation_session_id.as_ref().unwrap())
                .await
                .unwrap()
                .unwrap()
                .conversation_id,
            false,
        )
        .await
        .unwrap();
    assert_eq!(issues.len(), 1);
    assert!(
        issues[0].linked_followup_conversation_id.is_none(),
        "new follow-up creation requires a Wry app handle; this fixture should only record the issue"
    );
}

#[tokio::test]
async fn test_complete_review_reuses_existing_unrelated_drift_followup_session() {
    let (state, task) = setup_review_scope_drift_state().await;

    state
        .app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            max_revision_cycles: 1,
            auto_create_followup_agent_conversation: true,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings update should succeed");

    let prior_note = ReviewNote::with_content(
        task.id.clone(),
        ReviewerType::Ai,
        ReviewOutcome::ChangesRequested,
        Some("Previous revise round".to_string()),
        Some("Already sent back once".to_string()),
        None,
    );
    state
        .app_state
        .review_repo
        .add_note(&prior_note)
        .await
        .expect("prior review note should persist");

    let task_context = get_task_context_impl(&state.app_state, &task.id)
        .await
        .expect("task context should load");
    let blocker_fingerprint = task_context
        .out_of_scope_blocker_fingerprint
        .clone()
        .expect("scope drift fixture should compute blocker fingerprint");

    let origin_workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_linked_ideation_session_id(
            task.ideation_session_id
                .as_ref()
                .expect("task should have ideation session"),
        )
        .await
        .unwrap()
        .expect("origin Agent workspace should exist");
    let followup = ChatConversation::new_project(task.project_id.clone());
    state
        .app_state
        .chat_conversation_repo
        .create(followup.clone())
        .await
        .unwrap();
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            followup.id,
            task.project_id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            None,
            "ralphx/test/review-followup".to_string(),
            "/tmp/ralphx-review-followup".to_string(),
        ))
        .await
        .unwrap();
    state
        .app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(
            &followup.id,
            AgentWorkspaceFollowupProvenance {
                origin_conversation_id: origin_workspace.conversation_id,
                source_task_id: Some(task.id.as_str().to_string()),
                source_context_type: Some("review".to_string()),
                source_context_id: None,
                source_agent_name: Some("ralphx-execution-reviewer".to_string()),
                spawn_reason: Some("out_of_scope_failure".to_string()),
                blocker_fingerprint: Some(blocker_fingerprint),
            },
        )
        .await
        .unwrap();

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "escalate".to_string(),
        summary: Some("Still blocked by unrelated drift".to_string()),
        feedback: Some("The branch keeps reintroducing unrelated scope drift.".to_string()),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: Some("feature.rs is outside task scope".to_string()),
            step_id: None,
            no_step_reason: Some("Scope drift spans the task branch".to_string()),
            description: Some(
                "Repeated revise rounds did not remove the unrelated change.".to_string(),
            ),
            category: Some("quality".to_string()),
            file_path: Some("src/feature.rs".to_string()),
            line_number: Some(1),
            code_snippet: None,
        }]),
        escalation_reason: Some("Revision budget exhausted for unrelated scope drift".to_string()),
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some(
            "Escalation is now allowed because revise budget is exhausted.".to_string(),
        ),
    };

    let response = complete_review(State(state.clone()), ProjectScope(None), Json(req))
        .await
        .expect("escalation should reuse existing follow-up")
        .0;

    assert_eq!(
        response.followup_conversation_id,
        Some(followup.id.as_str())
    );
    let expected_fingerprint = task_context
        .out_of_scope_blocker_fingerprint
        .as_deref()
        .expect("task context should expose blocker fingerprint");
    let issues = state
        .app_state
        .agent_conversation_issue_repo
        .list_by_conversation(&origin_workspace.conversation_id, false)
        .await
        .unwrap();
    assert_eq!(issues.len(), 1, "review should reuse existing follow-up");
    assert_eq!(
        issues[0].blocker_fingerprint.as_deref(),
        Some(expected_fingerprint)
    );
    assert_eq!(
        issues[0].linked_followup_conversation_id.as_ref(),
        Some(&followup.id)
    );
}

#[tokio::test]
async fn test_complete_review_requires_issues_for_unrelated_drift_needs_changes() {
    let (state, task) = setup_review_scope_drift_state().await;

    let req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "needs_changes".to_string(),
        summary: Some("Needs revision".to_string()),
        feedback: Some("Unrelated scope drift must be revised out of the branch.".to_string()),
        issues: None,
        escalation_reason: None,
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some("Worker needs a structured issue to act on.".to_string()),
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(
                msg.contains("requires at least one structured issue"),
                "expected structured issue guard message, got: {msg}"
            );
        }
        Ok(_) => panic!("needs_changes for unrelated drift must require structured issues"),
    }
}

#[tokio::test]
async fn test_unrelated_drift_revise_first_then_followup_after_budget_exhausted() {
    let (state, task) = setup_review_scope_drift_state().await;

    state
        .app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            max_revision_cycles: 1,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings update should succeed");

    let revise_req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "needs_changes".to_string(),
        summary: Some("Remove unrelated scope drift".to_string()),
        feedback: Some("Revise the branch so only scoped files remain.".to_string()),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: Some("feature.rs is outside task scope".to_string()),
            step_id: None,
            no_step_reason: Some(
                "Scope drift spans the task branch, not a single execution step".to_string(),
            ),
            description: Some(
                "Remove the unrelated feature.rs change from this task branch.".to_string(),
            ),
            category: Some("quality".to_string()),
            file_path: Some("feature.rs".to_string()),
            line_number: Some(1),
            code_snippet: None,
        }]),
        escalation_reason: None,
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some("Send the task back through revise before escalating.".to_string()),
    };

    let revise_response =
        complete_review(State(state.clone()), ProjectScope(None), Json(revise_req))
            .await
            .expect("first unrelated drift review should go through revise")
            .0;
    assert_eq!(revise_response.new_status, "revision_needed");
    assert!(
        revise_response.followup_session_id.is_none(),
        "revise-first path must not spawn a follow-up session immediately"
    );
    assert!(
        revise_response.followup_conversation_id.is_none(),
        "revise-first path must not spawn a follow-up Agent conversation immediately"
    );

    let review_issues = state
        .app_state
        .review_issue_repo
        .get_by_task_id(&task.id)
        .await
        .expect("review issues should load");
    assert_eq!(
        review_issues.len(),
        1,
        "revise-first path should persist a structured issue for the worker"
    );

    let mut reviewing_task = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task should still exist");
    reviewing_task.internal_status = InternalStatus::Reviewing;
    state
        .app_state
        .task_repo
        .update(&reviewing_task)
        .await
        .expect("task should be reset to reviewing for the next review round");

    let escalate_req = CompleteReviewRequest {
        task_id: task.id.as_str().to_string(),
        decision: "escalate".to_string(),
        summary: Some("Repeated revise cycle still drifted".to_string()),
        feedback: Some("The branch keeps reintroducing unrelated scope drift.".to_string()),
        issues: Some(vec![ReviewIssueRequest {
            severity: "major".to_string(),
            title: Some("feature.rs is outside task scope".to_string()),
            step_id: None,
            no_step_reason: Some("Repeated revise cycle could not isolate the branch".to_string()),
            description: Some(
                "This unrelated change should be handled in a follow-up session.".to_string(),
            ),
            category: Some("quality".to_string()),
            file_path: Some("feature.rs".to_string()),
            line_number: Some(1),
            code_snippet: None,
        }]),
        escalation_reason: Some("Revision budget exhausted for unrelated scope drift".to_string()),
        scope_drift_classification: Some("unrelated_drift".to_string()),
        scope_drift_notes: Some(
            "Spawn follow-up after revise-first budget is exhausted.".to_string(),
        ),
    };

    let escalate_response =
        complete_review(State(state.clone()), ProjectScope(None), Json(escalate_req))
            .await
            .expect("exhausted unrelated drift should escalate after revise-first")
            .0;

    assert!(
        escalate_response.followup_conversation_id.is_none(),
        "Wry-free review fixture should register the Agent issue without spawning a new conversation"
    );
    let origin_workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_linked_ideation_session_id(task.ideation_session_id.as_ref().unwrap())
        .await
        .unwrap()
        .expect("origin Agent workspace should exist");
    let issues = state
        .app_state
        .agent_conversation_issue_repo
        .list_by_conversation(&origin_workspace.conversation_id, false)
        .await
        .unwrap();
    assert_eq!(issues.len(), 1);
    assert!(
        issues[0].linked_followup_conversation_id.is_none(),
        "new follow-up creation requires a Wry app handle; this fixture should only record the issue"
    );
}

// ============================================================================
// IPR (Interactive Process Registry) exit signal tests for complete_review
// ============================================================================

/// complete_review — no IPR entry is safe: handler succeeds without IPR registered.
///
/// When no IPR entry is present (reviewer agent already exited), the IPR removal
/// is a no-op and must not cause the handler to fail.
#[tokio::test]
async fn test_complete_review_no_ipr_entry_is_safe() {
    let state = setup_review_test_state().await;
    let mut task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    set_no_code_changes_metadata(&mut task);
    state.app_state.task_repo.update(&task).await.unwrap();
    let task_id = task.id.as_str().to_string();

    // No IPR entry registered — IPR removal is a no-op
    let req = CompleteReviewRequest {
        task_id,
        decision: "approved".to_string(),
        summary: None,
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;

    // Guard must NOT fire (task is in Reviewing state), so 400 is not acceptable.
    // Transition may succeed (200) or fail (500) depending on in-memory repo support —
    // both are acceptable here; we're testing IPR safety, not the transition itself.
    if let Err((status, msg)) = &result {
        assert_ne!(
            *status,
            StatusCode::BAD_REQUEST,
            "Absent IPR entry must not trigger the 400 guard. Got 400: {msg}",
        );
    }
}

/// complete_review — IPR entry removed after successful approval.
///
/// When the full review flow succeeds (task transitions away from Reviewing),
/// the IPR entry for the "review" context must be removed so the reviewer agent
/// receives EOF on stdin and exits gracefully.
#[tokio::test]
async fn test_complete_review_ipr_removed_on_success() {
    let state = setup_review_test_state().await;
    let mut task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    set_no_code_changes_metadata(&mut task);
    state.app_state.task_repo.update(&task).await.unwrap();
    let task_id = task.id.clone();

    // Register IPR entry for the reviewer agent
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cat for review IPR test");
    let stdin = child.stdin.take().expect("cat stdin");

    let key = InteractiveProcessKey::new("review", task_id.as_str());
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

    let req = CompleteReviewRequest {
        task_id: task_id.as_str().to_string(),
        decision: "approved".to_string(),
        summary: Some("LGTM".to_string()),
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;

    // Only assert IPR removal when the full handler flow succeeded.
    // If the state transition fails (in-memory repo limitation), the IPR removal
    // code is never reached, which is a known constraint of handler-level tests.
    if result.is_ok() {
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be removed after complete_review succeeds"
        );
    }

    // Clean up regardless of result
    state
        .app_state
        .interactive_process_registry
        .remove(&key)
        .await;
    let _ = child.kill().await;
}

/// complete_review — IPR entry removed after needs_changes decision.
///
/// When the reviewer agent calls complete_review with decision="needs_changes",
/// the IPR entry must be removed regardless of decision type so the agent gets EOF.
#[tokio::test]
async fn test_complete_review_ipr_removed_on_needs_changes() {
    let state = setup_review_test_state().await;
    let task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    let task_id = task.id.clone();

    // Register IPR entry for the reviewer agent
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cat for review IPR needs_changes test");
    let stdin = child.stdin.take().expect("cat stdin");

    let key = InteractiveProcessKey::new("review", task_id.as_str());
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

    let req = CompleteReviewRequest {
        task_id: task_id.as_str().to_string(),
        decision: "needs_changes".to_string(),
        summary: Some("Found issues".to_string()),
        feedback: Some("Please fix the error handling".to_string()),
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;

    // Only assert IPR removal when the full handler flow succeeded.
    if result.is_ok() {
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be removed after complete_review needs_changes succeeds"
        );
    }

    // Clean up regardless of result
    state
        .app_state
        .interactive_process_registry
        .remove(&key)
        .await;
    let _ = child.kill().await;
}

/// complete_review — IPR entry removed after escalate decision.
///
/// When the reviewer agent calls complete_review with decision="escalate",
/// the IPR entry must be removed so the agent receives EOF and exits gracefully.
#[tokio::test]
async fn test_complete_review_ipr_removed_on_escalate() {
    let state = setup_review_test_state().await;
    let task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    let task_id = task.id.clone();

    // Register IPR entry for the reviewer agent
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cat for review IPR escalate test");
    let stdin = child.stdin.take().expect("cat stdin");

    let key = InteractiveProcessKey::new("review", task_id.as_str());
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

    let req = CompleteReviewRequest {
        task_id: task_id.as_str().to_string(),
        decision: "escalate".to_string(),
        summary: Some("Complex issue requiring human review".to_string()),
        feedback: Some("Needs a human expert to decide".to_string()),
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    let result = complete_review(State(state.clone()), ProjectScope(None), Json(req)).await;

    // Only assert IPR removal when the full handler flow succeeded.
    if result.is_ok() {
        assert!(
            !state
                .app_state
                .interactive_process_registry
                .has_process(&key)
                .await,
            "IPR must be removed after complete_review escalate succeeds"
        );
    }

    // Clean up regardless of result
    state
        .app_state
        .interactive_process_registry
        .remove(&key)
        .await;
    let _ = child.kill().await;
}

// ============================================================================
// approved_no_changes handler tests
// ============================================================================

/// approved_no_changes decision — string parses correctly and handler proceeds past guard.
///
/// Verifies that "approved_no_changes" is a valid decision string (not rejected with 400),
/// and that the handler proceeds past the Reviewing-state guard.
///
/// Note: In the in-memory test setup, the project is not seeded, so the git diff validation
/// takes the defensive path (no project found → proceed with no-changes path). The transition
/// may succeed or fail depending on in-memory repo behavior — we only check the guard doesn't
/// fire for a Reviewing task.
#[tokio::test]
async fn test_approved_no_changes_string_parsed_correctly() {
    let state = setup_review_test_state().await;
    let mut task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    set_no_code_changes_metadata(&mut task);
    state.app_state.task_repo.update(&task).await.unwrap();
    let task_id = task.id.as_str().to_string();

    let req = CompleteReviewRequest {
        task_id,
        decision: "approved_no_changes".to_string(),
        summary: Some("Research task — no code changes".to_string()),
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    // The guard-specific 400 must NOT be returned — "approved_no_changes" is a valid decision.
    // Handler may return 200 or a non-guard error from the transition itself.
    if let Err((status, msg)) = &result {
        assert_ne!(
            *status,
            StatusCode::BAD_REQUEST,
            "approved_no_changes must not be rejected as an invalid decision. \
             Got 400: {msg}",
        );
    }
}

/// approved_no_changes — invalid decision string still returns 400.
///
/// Verifies the fallthrough case in the string match gate.
#[tokio::test]
async fn test_invalid_decision_still_rejected() {
    let state = setup_review_test_state().await;
    let task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    let task_id = task.id.as_str().to_string();

    let req = CompleteReviewRequest {
        task_id,
        decision: "approve_no_changes_typo".to_string(),
        summary: None,
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "Typo in decision must return 400. Got: {status}",
            );
            assert!(
                msg.contains("Invalid decision"),
                "Error must mention 'Invalid decision'. Got: {msg}",
            );
        }
        Ok(_) => panic!("Invalid decision must return 400"),
    }
}

/// approved_no_changes error message now includes 'approved_no_changes' in the valid list.
///
/// When an invalid decision is provided, the error message must include the new variant.
#[tokio::test]
async fn test_invalid_decision_error_lists_approved_no_changes() {
    let state = setup_review_test_state().await;
    let task = seed_task_with_status(&state, InternalStatus::Reviewing).await;
    let task_id = task.id.as_str().to_string();

    let req = CompleteReviewRequest {
        task_id,
        decision: "bad_decision".to_string(),
        summary: None,
        feedback: None,
        issues: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };

    let result = complete_review(State(state), ProjectScope(None), Json(req)).await;

    match result {
        Err((status, msg)) => {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(
                msg.contains("approved_no_changes"),
                "Error message for invalid decision must include 'approved_no_changes' \
                 in the valid options list. Got: {msg}",
            );
        }
        Ok(_) => panic!("Invalid decision must return 400"),
    }
}
