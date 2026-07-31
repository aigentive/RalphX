use std::{env, fs, process::Command, sync::Arc};

use std::os::unix::fs::PermissionsExt;

use axum::{
    body::{to_bytes, Body},
    extract::{Path, State},
    http::{HeaderMap, Request, StatusCode},
    routing::post,
    Json, Router,
};
use chrono::{Duration, Utc};
use ralphx_lib::application::{
    agent_conversation_workspace::resolve_agent_conversation_workspace_path, AppState, GitService,
};
use ralphx_lib::commands::{
    unified_chat_commands::install_agent_workspace_repair_publish_continuation, ExecutionState,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunId,
    AgentRunStatus, AgentWorkspaceRepairAttempt, AgentWorkspaceRepairOutcome,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, ChatConversationId, GitTargetLeaseOwner,
    IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use ralphx_lib::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, BindAgentWorkspaceRepairAttemptRun,
    SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use ralphx_lib::http_server::handlers::agent_workspaces::{
    clear_agent_workspace_repair_completion_blocker_gate_for_test,
    clear_agent_workspace_repair_completion_continuation_gate_for_test,
    clear_agent_workspace_repair_completion_reservation_gate_for_test,
    clear_agent_workspace_repair_completion_success_reservation_gate_for_test,
    clear_agent_workspace_repair_completion_validation_gate_for_test,
    complete_agent_workspace_pr_fix, complete_agent_workspace_repair,
    get_agent_workspace_publish_status,
    set_agent_workspace_repair_completion_blocker_gate_for_test,
    set_agent_workspace_repair_completion_continuation_gate_for_test,
    set_agent_workspace_repair_completion_reservation_gate_for_test,
    set_agent_workspace_repair_completion_success_reservation_gate_for_test,
    set_agent_workspace_repair_completion_validation_gate_for_test,
    CompleteAgentWorkspacePrFixRequest, CompleteAgentWorkspaceRepairRequest,
};
use ralphx_lib::http_server::types::HttpServerState;
use tower::ServiceExt;

fn test_state() -> HttpServerState {
    let app_state = AppState::new_test();
    install_agent_workspace_repair_publish_continuation(&app_state);
    HttpServerState {
        app_state: Arc::new(app_state),
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
    }
}

fn repair_completion_app(state: HttpServerState) -> Router {
    Router::new()
        .route(
            "/api/agent-workspaces/:conversation_id/complete-repair",
            post(complete_agent_workspace_repair),
        )
        .with_state(state)
}

fn pr_fix_compatibility_app(state: HttpServerState) -> Router {
    Router::new()
        .route(
            "/api/agent-workspaces/:conversation_id/complete-pr-fix",
            post(complete_agent_workspace_pr_fix),
        )
        .with_state(state)
}

async fn repair_completion_http_response(
    state: HttpServerState,
    conversation_id: &ChatConversationId,
    headers: HeaderMap,
    request: CompleteAgentWorkspaceRepairRequest,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/agent-workspaces/{}/complete-repair",
            conversation_id
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "summary": request.summary,
                "blocker": request.blocker,
            }))
            .expect("serialize repair completion request"),
        ))
        .expect("build repair completion request");
    request.headers_mut().extend(headers);
    repair_completion_app(state)
        .oneshot(request)
        .await
        .expect("repair completion router response")
}

async fn pr_fix_compatibility_http_response(
    state: HttpServerState,
    conversation_id: &ChatConversationId,
    request: CompleteAgentWorkspacePrFixRequest,
) -> axum::response::Response {
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/agent-workspaces/{}/complete-pr-fix",
            conversation_id
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "summary": request.summary,
                "blocker": request.blocker,
                "fix_commit_sha": request.fix_commit_sha,
                "created_by_run_id": request.created_by_run_id,
            }))
            .expect("serialize PR-fix compatibility request"),
        ))
        .expect("build PR-fix compatibility request");
    pr_fix_compatibility_app(state)
        .oneshot(request)
        .await
        .expect("PR-fix compatibility router response")
}

async fn response_status(response: axum::response::Response) -> (StatusCode, String) {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read repair completion response body");
    let status_name = serde_json::from_slice::<serde_json::Value>(&body)
        .expect("repair completion response JSON")
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    (status, status_name)
}

async fn assert_transport_authority_rejection(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    before: &AgentWorkspaceRepairAttempt,
    response: axum::response::Response,
    expected_status: StatusCode,
) {
    let (status, response_status) = response_status(response).await;
    assert_eq!(status, expected_status);
    assert!(
        status.is_client_error(),
        "invalid runtime authority must not be a successful transport response"
    );
    assert!(
        response_status.is_empty(),
        "transport authorization errors must use the established JSON error envelope"
    );

    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("read current repair attempt")
        .expect("current repair attempt remains");
    assert_eq!(after.id, before.id);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.summary, before.summary);
    assert_eq!(after.blocker, before.blocker);
    assert_no_duplicate_completion_side_effects(state, conversation_id, &after).await;
}

fn completion_headers(conversation_id: ChatConversationId, run_id: AgentRunId) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        run_id.to_string().parse().expect("run header"),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        conversation_id
            .to_string()
            .parse()
            .expect("conversation header"),
    );
    headers
}

fn run_git(repo_path: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn seed_current_attempt(
    state: &HttpServerState,
) -> (ChatConversationId, AgentRunId, AgentWorkspaceRepairAttempt) {
    let conversation_id = ChatConversationId::new();
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("repair-completion-project".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-head".to_string()),
        "ralphx/test/repair-completion".to_string(),
        "/missing-on-purpose".to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    bind_current_attempt(state, conversation_id).await
}

async fn bind_current_attempt(
    state: &HttpServerState,
    conversation_id: ChatConversationId,
) -> (ChatConversationId, AgentRunId, AgentWorkspaceRepairAttempt) {
    let attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::BaseUpdate,
        ralphx_lib::domain::entities::AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    let started = state
        .app_state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "repair completion authority test".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first repair attempt must start");
    };
    let owner_run = AgentRun::new(conversation_id);
    let owner_run_id = owner_run.id;
    state
        .app_state
        .agent_run_repo
        .create(owner_run)
        .await
        .expect("seed owner run");
    let bound = state
        .app_state
        .agent_workspace_repair_repo
        .bind_repair_attempt_run(BindAgentWorkspaceRepairAttemptRun {
            attempt_id: started.id.clone(),
            generation: started.generation,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: started.updated_at,
            run_id: owner_run_id,
            runtime_conversation_id: None,
            updated_at: Utc::now(),
        })
        .await
        .expect("bind owner run");
    let ralphx_lib::domain::repositories::AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
        bound,
    ) = bound
    else {
        panic!("repair owner run must bind");
    };
    (conversation_id, owner_run_id, bound)
}

async fn seed_current_attempt_with_resolvable_target(
    state: &HttpServerState,
) -> (
    ChatConversationId,
    AgentRunId,
    AgentWorkspaceRepairAttempt,
    tempfile::TempDir,
) {
    let root = tempfile::TempDir::new().expect("temporary repair workspace root");
    let project_root = root.path().join("project");
    let worktree_parent = root.path().join("worktrees");
    fs::create_dir_all(&project_root).expect("create project root");
    let conversation_id = ChatConversationId::new();
    let mut project = Project::new(
        "Repair completion reservation".to_string(),
        project_root.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("repair-completion-reservation-project".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("derive deterministic workspace path");
    fs::create_dir_all(&worktree_path).expect("create deterministic workspace path");
    fs::write(
        worktree_path.join(".git"),
        "gitdir: /not-used-by-this-test\n",
    )
    .expect("mark deterministic workspace as a git worktree");
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            conversation_id,
            project.id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-head".to_string()),
            "ralphx/test/repair-completion".to_string(),
            worktree_path.to_string_lossy().to_string(),
        ))
        .await
        .expect("seed deterministic repair workspace");
    let (conversation_id, run_id, attempt) = bind_current_attempt(state, conversation_id).await;
    (conversation_id, run_id, attempt, root)
}

async fn seed_current_attempt_with_valid_target(
    state: &HttpServerState,
) -> (
    ChatConversationId,
    AgentRunId,
    AgentWorkspaceRepairAttempt,
    tempfile::TempDir,
) {
    let root = tempfile::TempDir::new().expect("temporary repair workspace root");
    let project_root = root.path().join("project");
    let origin_path = root.path().join("origin.git");
    let worktree_parent = root.path().join("worktrees");
    let origin = origin_path.to_string_lossy().to_string();
    fs::create_dir_all(&project_root).expect("create project root");
    run_git(root.path(), &["init", "--bare", &origin]);
    run_git(&project_root, &["init", "--initial-branch=main"]);
    run_git(
        &project_root,
        &["config", "user.email", "repair@example.test"],
    );
    run_git(
        &project_root,
        &["config", "user.name", "Repair Completion Test"],
    );
    fs::write(
        project_root.join("README.md"),
        "repair completion fixture\n",
    )
    .expect("write initial repository file");
    run_git(&project_root, &["add", "README.md"]);
    run_git(
        &project_root,
        &["commit", "-m", "initial repair completion fixture"],
    );
    run_git(&project_root, &["remote", "add", "origin", &origin]);
    run_git(&project_root, &["push", "-u", "origin", "main"]);

    let conversation_id = ChatConversationId::new();
    let mut project = Project::new(
        "Repair completion success".to_string(),
        project_root.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("repair-completion-success-project".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("derive deterministic workspace path");
    let worktree = worktree_path.to_string_lossy().to_string();
    run_git(
        &project_root,
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/test/repair-completion",
            &worktree,
            "main",
        ],
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            conversation_id,
            project.id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-head".to_string()),
            "ralphx/test/repair-completion".to_string(),
            worktree,
        ))
        .await
        .expect("seed deterministic repair workspace");
    let (conversation_id, run_id, attempt) = bind_current_attempt(state, conversation_id).await;
    let attempt = checkpoint_current_attempt_target_lease(state, attempt).await;
    (conversation_id, run_id, attempt, root)
}

async fn checkpoint_current_attempt_target_lease(
    state: &HttpServerState,
    attempt: AgentWorkspaceRepairAttempt,
) -> AgentWorkspaceRepairAttempt {
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await
        .expect("load repair workspace")
        .expect("repair workspace exists");
    let identity = GitService::canonical_target_identity(
        std::path::Path::new(&workspace.worktree_path),
        &workspace.branch_name,
    )
    .await
    .expect("resolve canonical repair target");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = state
        .app_state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner,
        })
        .await
        .expect("acquire repair target lease")
    else {
        panic!("repair target lease must be newly acquired");
    };

    let mut checkpointed = attempt.clone();
    checkpointed.phase = AgentWorkspaceRepairPhase::Repairing;
    checkpointed.git_common_dir = Some(identity.git_common_dir().to_string_lossy().into_owned());
    checkpointed.target_ref = Some(identity.full_ref().to_string());
    checkpointed.target_identity_version = Some(1);
    checkpointed.target_lease_epoch = Some(fencing_epoch);
    checkpointed.updated_at += Duration::microseconds(1);
    let checkpointed = state
        .app_state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: checkpointed,
            expected_phase: attempt.phase,
            expected_updated_at: attempt.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint repair target lease");
    let AgentWorkspaceRepairAttemptTransitionOutcome::Applied(checkpointed) = checkpointed else {
        panic!("repair target lease checkpoint must apply");
    };
    checkpointed
}

async fn assert_no_duplicate_completion_side_effects(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    attempt: &AgentWorkspaceRepairAttempt,
) {
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(conversation_id)
            .await
            .expect("read repair publication events")
            .is_empty(),
        "duplicate completion must not append audit events"
    );
    assert!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&attempt.id)
            .await
            .expect("read open repair effect")
            .is_none(),
        "duplicate completion must not start a repair effect"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(conversation_id)
            .await
            .expect("read workspace messages")
            .is_empty(),
        "duplicate completion must not emit a chat message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "duplicate completion must not enqueue a message"
    );
}

async fn block_valid_current_attempt(
    state: &HttpServerState,
    conversation_id: ChatConversationId,
    run_id: AgentRunId,
) -> AgentWorkspaceRepairAttempt {
    let Json(response) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The repair needs an explicit maintainer decision.".to_string(),
            blocker: Some("Choose the safe repair path before continuing.".to_string()),
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    .expect("block the valid repair attempt");
    assert_eq!(response.status, "blocked");
    let blocked = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read blocked repair attempt")
        .expect("blocked repair attempt remains current");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(blocked.target_lease_epoch.is_none());
    blocked
}

#[tokio::test]
async fn legacy_pr_fix_transport_without_a_durable_attempt_fails_closed_without_legacy_effects() {
    let state = test_state();
    let conversation_id = ChatConversationId::new();
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("legacy-pr-fix-no-attempt".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-head".to_string()),
        "ralphx/test/legacy-pr-fix-no-attempt".to_string(),
        "/missing-on-purpose".to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace without a durable attempt");
    let before = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read seeded workspace")
        .expect("workspace remains");
    let owner_run = AgentRun::new(conversation_id);
    let owner_run_id = owner_run.id;
    state
        .app_state
        .agent_run_repo
        .create(owner_run)
        .await
        .expect("seed trusted transport run");

    let response = pr_fix_compatibility_http_response(
        state.clone(),
        &conversation_id,
        CompleteAgentWorkspacePrFixRequest {
            summary: "Legacy completion must not create repair authority".to_string(),
            blocker: None,
            fix_commit_sha: None,
            created_by_run_id: Some(owner_run_id.to_string()),
            resolution: None,
        },
    )
    .await;
    let (status, response_status) = response_status(response).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(response_status.is_empty());
    assert!(state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read durable attempt")
        .is_none());
    let after = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload workspace")
        .expect("workspace remains");
    assert_eq!(after.updated_at, before.updated_at);
    assert!(state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events")
        .is_empty());
    assert!(state
        .app_state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("list chat messages")
        .is_empty());
    assert!(state.app_state.message_queue.list_keys().is_empty());
}

#[tokio::test]
async fn legacy_pr_fix_transport_rejects_wrong_durable_run_without_effects() {
    let state = test_state();
    let (conversation_id, _owner_run_id, before) = seed_current_attempt(&state).await;
    let wrong_run = AgentRun::new(conversation_id);
    let wrong_run_id = wrong_run.id;
    state
        .app_state
        .agent_run_repo
        .create(wrong_run)
        .await
        .expect("seed wrong transport run");

    let response = pr_fix_compatibility_http_response(
        state.clone(),
        &conversation_id,
        CompleteAgentWorkspacePrFixRequest {
            summary: "Wrong repair run must not complete the durable attempt".to_string(),
            blocker: None,
            fix_commit_sha: None,
            created_by_run_id: Some(wrong_run_id.to_string()),
            resolution: None,
        },
    )
    .await;
    let (status, response_status) = response_status(response).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(response_status.is_empty());
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload durable attempt")
        .expect("durable attempt remains");
    assert_eq!(after.id, before.id);
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.updated_at, before.updated_at);
    assert_no_duplicate_completion_side_effects(&state, &conversation_id, &after).await;
}

#[tokio::test]
async fn legacy_pr_fix_transport_uses_durable_completion_without_legacy_publish_writers() {
    let state = test_state();
    let (conversation_id, owner_run_id, before) = seed_current_attempt(&state).await;

    let response = pr_fix_compatibility_http_response(
        state.clone(),
        &conversation_id,
        CompleteAgentWorkspacePrFixRequest {
            summary: "Durable coordinator owns this compatibility completion".to_string(),
            blocker: None,
            fix_commit_sha: Some("f".repeat(40)),
            created_by_run_id: Some(owner_run_id.to_string()),
            resolution: None,
        },
    )
    .await;
    let (status, response_status) = response_status(response).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(response_status.is_empty());
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("reload durable attempt")
        .expect("durable attempt remains");
    assert_eq!(after.id, before.id);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list compatibility events")
        .iter()
        .all(|event| !event.step.starts_with("pr_autofix_")));
    assert!(state
        .app_state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("list chat messages")
        .is_empty());
    assert!(state.app_state.message_queue.list_keys().is_empty());
    assert!(state
        .app_state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&after.id)
        .await
        .expect("read repair effect")
        .is_none());
}

#[test]
fn completion_request_accepts_only_summary_and_optional_blocker() {
    let request: CompleteAgentWorkspaceRepairRequest = serde_json::from_value(serde_json::json!({
        "summary": "Resolved the conflicts",
        "blocker": "Awaiting maintainer choice",
    }))
    .expect("summary and blocker are the full model-facing contract");
    assert_eq!(request.summary, "Resolved the conflicts");
    assert_eq!(
        request.blocker.as_deref(),
        Some("Awaiting maintainer choice")
    );

    let legacy = serde_json::from_value::<CompleteAgentWorkspaceRepairRequest>(serde_json::json!({
        "summary": "Resolved the conflicts",
        "repair_commit_sha": "a".repeat(40),
    }));
    assert!(legacy.is_err(), "model input must not carry Git authority");
}

#[tokio::test]
async fn transport_authority_rejects_missing_runtime_headers_without_mutation() {
    let state = test_state();
    let (conversation_id, _owner_run_id, before) = seed_current_attempt(&state).await;

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        HeaderMap::new(),
        CompleteAgentWorkspaceRepairRequest {
            summary: "Missing runtime identity must never settle a repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;

    assert_transport_authority_rejection(
        &state,
        &conversation_id,
        &before,
        response,
        StatusCode::UNAUTHORIZED,
    )
    .await;
}

#[tokio::test]
async fn transport_authority_rejects_malformed_runtime_run_without_mutation() {
    let state = test_state();
    let (conversation_id, _owner_run_id, before) = seed_current_attempt(&state).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        "not-a-runtime-run-id"
            .parse()
            .expect("malformed run header"),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        conversation_id
            .to_string()
            .parse()
            .expect("conversation header"),
    );

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        headers,
        CompleteAgentWorkspaceRepairRequest {
            summary: "Malformed runtime identity must never settle a repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;

    assert_transport_authority_rejection(
        &state,
        &conversation_id,
        &before,
        response,
        StatusCode::UNAUTHORIZED,
    )
    .await;
}

#[tokio::test]
async fn transport_authority_rejects_header_conversation_mismatch_without_mutation() {
    let state = test_state();
    let (conversation_id, owner_run_id, before) = seed_current_attempt(&state).await;

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        completion_headers(ChatConversationId::new(), owner_run_id),
        CompleteAgentWorkspaceRepairRequest {
            summary: "A mismatched runtime conversation must never settle a repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;

    assert_transport_authority_rejection(
        &state,
        &conversation_id,
        &before,
        response,
        StatusCode::UNAUTHORIZED,
    )
    .await;
}

#[tokio::test]
async fn transport_authority_rejects_cross_conversation_runtime_run_without_mutation() {
    let state = test_state();
    let (conversation_id, _owner_run_id, before) = seed_current_attempt(&state).await;
    let cross_conversation_run = AgentRun::new(ChatConversationId::new());
    let cross_conversation_run_id = cross_conversation_run.id;
    state
        .app_state
        .agent_run_repo
        .create(cross_conversation_run)
        .await
        .expect("seed cross-conversation runtime run");

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        completion_headers(conversation_id, cross_conversation_run_id),
        CompleteAgentWorkspaceRepairRequest {
            summary: "A cross-conversation run must not settle this repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;

    assert_transport_authority_rejection(
        &state,
        &conversation_id,
        &before,
        response,
        StatusCode::CONFLICT,
    )
    .await;
}

#[tokio::test]
async fn transport_authority_rejects_nonowning_runtime_run_without_mutation() {
    let state = test_state();
    let (conversation_id, _owner_run_id, before) = seed_current_attempt(&state).await;
    let nonowning_run = AgentRun::new(conversation_id);
    let nonowning_run_id = nonowning_run.id;
    state
        .app_state
        .agent_run_repo
        .create(nonowning_run)
        .await
        .expect("seed nonowning runtime run");

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        completion_headers(conversation_id, nonowning_run_id),
        CompleteAgentWorkspaceRepairRequest {
            summary: "A nonowning run must not settle this repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;

    assert_transport_authority_rejection(
        &state,
        &conversation_id,
        &before,
        response,
        StatusCode::CONFLICT,
    )
    .await;
}

#[tokio::test]
async fn transport_authority_rejects_missing_current_run_row_without_mutation() {
    let state = test_state();
    let (conversation_id, owner_run_id, before) = seed_current_attempt(&state).await;
    state
        .app_state
        .agent_run_repo
        .delete(&owner_run_id)
        .await
        .expect("delete current repair run");

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        completion_headers(conversation_id, owner_run_id),
        CompleteAgentWorkspaceRepairRequest {
            summary: "A missing current run row must not settle a repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;

    assert_transport_authority_rejection(
        &state,
        &conversation_id,
        &before,
        response,
        StatusCode::CONFLICT,
    )
    .await;
}

#[tokio::test]
async fn transport_authority_rejects_nonrunning_current_run_without_mutation() {
    let state = test_state();
    let (conversation_id, owner_run_id, before) = seed_current_attempt(&state).await;
    state
        .app_state
        .agent_run_repo
        .update_status(&owner_run_id, AgentRunStatus::Completed)
        .await
        .expect("complete current repair run");

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        completion_headers(conversation_id, owner_run_id),
        CompleteAgentWorkspaceRepairRequest {
            summary: "A non-running current run must not settle a repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;

    assert_transport_authority_rejection(
        &state,
        &conversation_id,
        &before,
        response,
        StatusCode::CONFLICT,
    )
    .await;
}

#[tokio::test]
async fn transport_authority_keeps_semantic_repair_outcomes_successful() {
    let accepted_state = test_state();
    let (accepted_conversation, accepted_run, _attempt, _root) =
        seed_current_attempt_with_valid_target(&accepted_state).await;
    let (status, outcome) = response_status(
        repair_completion_http_response(
            accepted_state.clone(),
            &accepted_conversation,
            completion_headers(accepted_conversation, accepted_run),
            CompleteAgentWorkspaceRepairRequest {
                summary: "The repaired branch is clean and contains the current base.".to_string(),
                blocker: None,
                reported_fix_commit_sha: None,
                resolution: None,
            },
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome, "accepted");

    let already_completed_state = test_state();
    let (already_completed_conversation, already_completed_run, attempt) =
        seed_current_attempt(&already_completed_state).await;
    let mut completed = attempt.clone();
    completed.phase = AgentWorkspaceRepairPhase::Validating;
    completed.updated_at += Duration::microseconds(1);
    let completed = already_completed_state
        .app_state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: completed,
            expected_phase: attempt.phase,
            expected_updated_at: attempt.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Validating,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("reserve idempotent completion");
    assert!(matches!(
        completed,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    let (status, outcome) = response_status(
        repair_completion_http_response(
            already_completed_state,
            &already_completed_conversation,
            completion_headers(already_completed_conversation, already_completed_run),
            CompleteAgentWorkspaceRepairRequest {
                summary: "This duplicate completion is safely idempotent.".to_string(),
                blocker: None,
                reported_fix_commit_sha: None,
                resolution: None,
            },
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome, "already_completed");

    let blocked_state = test_state();
    let (blocked_conversation, blocked_run, _attempt) = seed_current_attempt(&blocked_state).await;
    let (status, outcome) = response_status(
        repair_completion_http_response(
            blocked_state.clone(),
            &blocked_conversation,
            completion_headers(blocked_conversation, blocked_run),
            CompleteAgentWorkspaceRepairRequest {
                summary: "The repair needs a maintainer decision.".to_string(),
                blocker: Some("Choose whether to preserve the legacy schema.".to_string()),
                reported_fix_commit_sha: None,
                resolution: None,
            },
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome, "blocked");

    let superseded_state = test_state();
    let (superseded_conversation, superseded_run, attempt) =
        seed_current_attempt(&superseded_state).await;
    let successor = AgentWorkspaceRepairAttempt::new(
        superseded_conversation,
        AgentWorkspaceRepairSource::Publish,
        ralphx_lib::domain::entities::AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    let successor = superseded_state
        .app_state
        .agent_workspace_repair_repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: attempt.id,
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_updated_at: attempt.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Superseded,
            settled_at: Utc::now(),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: successor,
                reason: "new repair generation owns the workspace".to_string(),
                verified_newer_base: true,
                compatibility_projection: None,
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start superseding repair generation");
    assert!(matches!(
        successor,
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(_)
    ));
    let (status, outcome) = response_status(
        repair_completion_http_response(
            superseded_state,
            &superseded_conversation,
            completion_headers(superseded_conversation, superseded_run),
            CompleteAgentWorkspaceRepairRequest {
                summary: "A superseded repair must not affect the successor.".to_string(),
                blocker: None,
                reported_fix_commit_sha: None,
                resolution: None,
            },
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome, "superseded");
}

#[tokio::test]
async fn unknown_run_is_rejected_before_any_git_probe_or_attempt_mutation() {
    let state = test_state();
    let (conversation_id, _owner_run_id, before) = seed_current_attempt(&state).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        AgentRunId::new().to_string().parse().expect("run header"),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        conversation_id
            .to_string()
            .parse()
            .expect("conversation header"),
    );

    let Err(response) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        headers,
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "This stale run must be harmless".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    else {
        panic!("unknown runtime authority must fail at the transport boundary");
    };

    assert_eq!(response.0, StatusCode::CONFLICT);
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read current attempt")
        .expect("current attempt remains");
    assert_eq!(after.id, before.id);
    assert_eq!(after.phase, before.phase);
    assert!(after.summary.is_none());
    assert!(after.blocker.is_none());
    assert_no_duplicate_completion_side_effects(&state, &conversation_id, &after).await;
}

#[tokio::test]
async fn stale_validation_reservation_returns_before_every_git_probe() {
    let state = test_state();
    let (conversation_id, owner_run_id, before, root) =
        seed_current_attempt_with_resolvable_target(&state).await;
    let fake_git_dir = root.path().join("fake-git");
    let git_call_log = root.path().join("git-calls.log");
    fs::create_dir_all(&fake_git_dir).expect("create fake git directory");
    let fake_git = fake_git_dir.join("git");
    fs::write(
        &fake_git,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$RALPHX_REPAIR_COMPLETION_GIT_LOG\"\nprintf '%s\\n' 'ralphx/test/repair-completion'\n",
    )
    .expect("write fake git command");
    let mut permissions = fs::metadata(&fake_git)
        .expect("read fake git permissions")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_git, permissions).expect("make fake git executable");
    let previous_path = env::var_os("PATH");
    let previous_git_log = env::var_os("RALPHX_REPAIR_COMPLETION_GIT_LOG");
    let mut path_entries = vec![fake_git_dir];
    if let Some(path) = previous_path.as_ref() {
        path_entries.extend(env::split_paths(path));
    }
    env::set_var(
        "PATH",
        env::join_paths(path_entries).expect("assemble fake git path"),
    );
    env::set_var("RALPHX_REPAIR_COMPLETION_GIT_LOG", &git_call_log);

    let reservation_gate = Arc::new(tokio::sync::Barrier::new(2));
    set_agent_workspace_repair_completion_reservation_gate_for_test(Arc::clone(&reservation_gate));
    let completion = tokio::spawn(complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, owner_run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The completion snapshot must not validate after replacement.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    ));
    reservation_gate.wait().await;

    let mut competing_attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::Publish,
        ralphx_lib::domain::entities::AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    competing_attempt.updated_at = before.updated_at + Duration::nanoseconds(1);
    state
        .app_state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: competing_attempt,
            reason: "concurrent repair metadata update".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("concurrent update changes the optimistic validation version");
    reservation_gate.wait().await;

    let Json(response) = completion
        .await
        .expect("completion task joins")
        .expect("stale completion is an idempotent response");
    clear_agent_workspace_repair_completion_reservation_gate_for_test();
    match previous_path {
        Some(path) => env::set_var("PATH", path),
        None => env::remove_var("PATH"),
    }
    match previous_git_log {
        Some(path) => env::set_var("RALPHX_REPAIR_COMPLETION_GIT_LOG", path),
        None => env::remove_var("RALPHX_REPAIR_COMPLETION_GIT_LOG"),
    }

    assert_eq!(response.status, "superseded");
    assert!(
        !git_call_log.exists(),
        "a stale validation reservation must not resolve the target or execute any Git probe"
    );
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read current attempt")
        .expect("current attempt remains");
    assert_eq!(after.id, before.id);
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.summary, before.summary);
    assert_eq!(after.blocker, before.blocker);
}

#[tokio::test]
async fn racing_success_handoff_for_the_same_run_is_already_completed_without_duplicate_effects() {
    let state = test_state();
    let (conversation_id, owner_run_id, _attempt, _root) =
        seed_current_attempt_with_valid_target(&state).await;
    let continuation_gate = Arc::new(tokio::sync::Barrier::new(2));
    set_agent_workspace_repair_completion_continuation_gate_for_test(Arc::clone(
        &continuation_gate,
    ));
    let completion = tokio::spawn(complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, owner_run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The repaired branch is clean and contains the current base.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    ));
    continuation_gate.wait().await;

    let mut handed_off = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read validation reservation")
        .expect("validation reservation remains current");
    assert_eq!(handed_off.phase, AgentWorkspaceRepairPhase::Validating);
    let expected_updated_at = handed_off.updated_at;
    handed_off.phase = AgentWorkspaceRepairPhase::ContinuationPending;
    handed_off.updated_at += Duration::nanoseconds(1);
    let advanced = state
        .app_state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: handed_off.clone(),
            expected_phase: AgentWorkspaceRepairPhase::Validating,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("advance the exact repair generation to its continuation handoff");
    assert!(matches!(
        advanced,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    continuation_gate.wait().await;

    let Json(response) = completion
        .await
        .expect("completion task joins")
        .expect("racing completion receives an idempotent response");
    clear_agent_workspace_repair_completion_continuation_gate_for_test();

    assert_eq!(response.status, "already_completed");
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read advanced repair generation")
        .expect("advanced repair generation remains current");
    assert_eq!(after.id, handed_off.id);
    assert_eq!(after.phase, AgentWorkspaceRepairPhase::ContinuationPending);
    assert_eq!(after.updated_at, handed_off.updated_at);
    assert_no_duplicate_completion_side_effects(&state, &conversation_id, &after).await;
}

#[tokio::test]
async fn racing_success_duplicates_for_the_same_run_skip_extra_git_and_complete_idempotently() {
    let state = test_state();
    let (conversation_id, owner_run_id, attempt, root) =
        seed_current_attempt_with_valid_target(&state).await;
    let fake_git_dir = root.path().join("duplicate-success-fake-git");
    let git_call_log = root.path().join("duplicate-success-git-calls.log");
    fs::create_dir_all(&fake_git_dir).expect("create fake git directory");
    let fake_git = fake_git_dir.join("git");
    fs::write(
        &fake_git,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$RALPHX_REPAIR_COMPLETION_GIT_LOG\"\nexit 1\n",
    )
    .expect("write fake git command");
    let mut permissions = fs::metadata(&fake_git)
        .expect("read fake git permissions")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_git, permissions).expect("make fake git executable");
    let previous_path = env::var_os("PATH");
    let previous_git_log = env::var_os("RALPHX_REPAIR_COMPLETION_GIT_LOG");
    let mut path_entries = vec![fake_git_dir];
    if let Some(path) = previous_path.as_ref() {
        path_entries.extend(env::split_paths(path));
    }
    env::set_var(
        "PATH",
        env::join_paths(path_entries).expect("assemble fake git path"),
    );
    env::set_var("RALPHX_REPAIR_COMPLETION_GIT_LOG", &git_call_log);
    let reservation_gate = Arc::new(tokio::sync::Barrier::new(3));
    let validation_gate = Arc::new(tokio::sync::Barrier::new(2));
    set_agent_workspace_repair_completion_success_reservation_gate_for_test(Arc::clone(
        &reservation_gate,
    ));
    set_agent_workspace_repair_completion_validation_gate_for_test(Arc::clone(&validation_gate));
    let request = || {
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The repaired branch is clean and contains the current base.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        })
    };
    let mut first = tokio::spawn(complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, owner_run_id),
        request(),
    ));
    let mut second = tokio::spawn(complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, owner_run_id),
        request(),
    ));
    reservation_gate.wait().await;
    reservation_gate.wait().await;
    validation_gate.wait().await;

    let first_was_duplicate = tokio::select! {
        result = &mut first => {
            let Json(response) = result
                .expect("first success task joins")
                .expect("first success completion responds");
            assert_eq!(response.status, "already_completed");
            true
        }
        result = &mut second => {
            let Json(response) = result
                .expect("second success task joins")
                .expect("second success completion responds");
            assert_eq!(response.status, "already_completed");
            false
        }
    };
    let validating = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read validation owner")
        .expect("validation owner remains current");
    assert_eq!(validating.phase, AgentWorkspaceRepairPhase::Validating);
    assert!(
        validating.updated_at > attempt.updated_at,
        "the winning completion must advance the validation reservation"
    );
    assert!(
        !git_call_log.exists(),
        "the duplicate completion must not execute a Git probe while the winner is paused"
    );
    assert_no_duplicate_completion_side_effects(&state, &conversation_id, &validating).await;

    match previous_path {
        Some(path) => env::set_var("PATH", path),
        None => env::remove_var("PATH"),
    }
    match previous_git_log {
        Some(path) => env::set_var("RALPHX_REPAIR_COMPLETION_GIT_LOG", path),
        None => env::remove_var("RALPHX_REPAIR_COMPLETION_GIT_LOG"),
    }

    validation_gate.wait().await;
    let Json(accepted) = if first_was_duplicate {
        second
            .await
            .expect("winning second success task joins")
            .expect("winning second completion responds")
    } else {
        first
            .await
            .expect("winning first success task joins")
            .expect("winning first completion responds")
    };
    clear_agent_workspace_repair_completion_success_reservation_gate_for_test();
    clear_agent_workspace_repair_completion_validation_gate_for_test();

    assert_eq!(accepted.status, "accepted");
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read accepted repair generation")
        .expect("accepted repair generation remains current");
    assert_no_duplicate_completion_side_effects(&state, &conversation_id, &after).await;
}

#[tokio::test]
async fn racing_blocker_duplicates_for_the_same_run_are_already_blocked_without_side_effects() {
    let state = test_state();
    let (conversation_id, owner_run_id, _attempt) = seed_current_attempt(&state).await;
    let blocker_gate = Arc::new(tokio::sync::Barrier::new(3));
    set_agent_workspace_repair_completion_blocker_gate_for_test(Arc::clone(&blocker_gate));
    let first = tokio::spawn(complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, owner_run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The repair needs an explicit schema choice.".to_string(),
            blocker: Some("Choose whether to preserve the legacy schema.".to_string()),
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    ));
    let second = tokio::spawn(complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, owner_run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The repair needs an explicit schema choice.".to_string(),
            blocker: Some("Choose whether to preserve the legacy schema.".to_string()),
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    ));
    blocker_gate.wait().await;
    blocker_gate.wait().await;

    let Json(first) = first
        .await
        .expect("first blocker task joins")
        .expect("first blocker completion responds");
    let Json(second) = second
        .await
        .expect("second blocker task joins")
        .expect("second blocker completion responds");
    clear_agent_workspace_repair_completion_blocker_gate_for_test();

    assert_eq!(first.status, "blocked");
    assert_eq!(second.status, "blocked");
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read blocked repair generation")
        .expect("blocked repair generation remains current");
    assert_eq!(after.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(
        after.blocker.as_deref(),
        Some("Choose whether to preserve the legacy schema.")
    );
    assert_no_duplicate_completion_side_effects(&state, &conversation_id, &after).await;
}

#[tokio::test]
async fn completion_from_a_superseded_generation_stays_superseded_without_side_effects() {
    let state = test_state();
    let (conversation_id, owner_run_id, attempt) = seed_current_attempt(&state).await;
    let successor = AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::Publish,
        ralphx_lib::domain::entities::AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    let successor = state
        .app_state
        .agent_workspace_repair_repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_updated_at: attempt.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Superseded,
            settled_at: Utc::now(),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: successor,
                reason: "new repair generation owns the workspace".to_string(),
                verified_newer_base: true,
                compatibility_projection: None,
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start successor generation");
    let SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(successor) = successor else {
        panic!("superseding the current generation must start a successor");
    };

    let Json(response) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, owner_run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "A superseded repair must not affect the next generation.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    .expect("superseded generation returns an idempotent response");

    assert_eq!(response.status, "superseded");
    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read successor generation")
        .expect("successor generation remains current");
    assert_eq!(current.id, successor.id);
    assert_eq!(current.generation, successor.generation);
    assert_no_duplicate_completion_side_effects(&state, &conversation_id, &current).await;
}

#[tokio::test]
async fn trusted_blocker_settles_the_generation_once_without_git_or_audit_side_effects() {
    let state = test_state();
    let (conversation_id, owner_run_id, before) = seed_current_attempt(&state).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        owner_run_id.to_string().parse().expect("run header"),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        conversation_id
            .to_string()
            .parse()
            .expect("conversation header"),
    );

    let Json(response) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        headers.clone(),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The repair cannot safely choose a migration path.".to_string(),
            blocker: Some("Choose whether to preserve the legacy schema.".to_string()),
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    .expect("trusted blocker completion");
    assert_eq!(response.status, "blocked");

    let blocked = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read blocked attempt")
        .expect("blocked attempt remains current");
    assert_eq!(blocked.id, before.id);
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(
        blocked.blocker.as_deref(),
        Some("Choose whether to preserve the legacy schema.")
    );
    assert!(state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read audit events")
        .is_empty());

    let Json(duplicate) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        headers,
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Duplicate blocker signal".to_string(),
            blocker: Some("Different stale blocker".to_string()),
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    .expect("duplicate blocker is idempotent");
    assert_eq!(duplicate.status, "blocked");
    assert_eq!(
        duplicate.message,
        "This repair generation is blocked. Retry repair from the workspace to start a new repair attempt."
    );
    let after_duplicate = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read blocked attempt after duplicate")
        .expect("blocked attempt remains current");
    assert_eq!(after_duplicate.id, blocked.id);
    assert_eq!(after_duplicate.updated_at, blocked.updated_at);
    assert_eq!(after_duplicate.blocker, blocked.blocker);
    assert!(state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read audit events after duplicate")
        .is_empty());
}

#[tokio::test]
async fn blocked_exact_run_with_clean_repair_resurrects_through_validation_and_continuation() {
    let state = test_state();
    let (conversation_id, run_id, attempt, _root) =
        seed_current_attempt_with_valid_target(&state).await;
    // An update-only continuation keeps the post-resurrection workflow off GitHub so this
    // fixture proves the resurrection itself; publish continuations are covered elsewhere.
    let mut update_only = attempt.clone();
    let expected_updated_at = update_only.updated_at;
    update_only.continuation =
        ralphx_lib::domain::entities::AgentWorkspaceRepairContinuation::UpdateOnly;
    update_only.updated_at += Duration::microseconds(1);
    let seeded_phase = update_only.phase;
    match state
        .app_state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: update_only,
            expected_phase: seeded_phase,
            expected_updated_at,
            next_phase: seeded_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("flip seeded continuation to update-only")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        other => panic!("expected update-only continuation flip, got {other:?}"),
    }
    let blocked = block_valid_current_attempt(&state, conversation_id, run_id).await;

    let Json(response) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The committed repair is clean at the durable target base.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    .expect("resurrect exact blocked repair run");

    assert_eq!(response.status, "accepted");
    let continued = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read continued repair attempt")
        .expect("continued repair attempt remains current");
    assert_eq!(continued.id, blocked.id);
    assert_ne!(continued.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(
        continued.phase,
        AgentWorkspaceRepairPhase::Ready,
        "clean update-only resurrection parks the repaired workspace at Ready: {continued:?}"
    );
    assert!(
        continued.repair_head_commit.is_some(),
        "resurrection validation records the committed repair head"
    );
    assert!(
        continued.blocker.is_none(),
        "no blocker after clean resurrection: {continued:?}"
    );
}

#[tokio::test]
async fn blocked_exact_run_with_unproven_repair_stays_blocked_without_continuation() {
    let state = test_state();
    let (conversation_id, run_id, _attempt, root) =
        seed_current_attempt_with_valid_target(&state).await;
    let blocked = block_valid_current_attempt(&state, conversation_id, run_id).await;
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read repair workspace")
        .expect("repair workspace exists");
    fs::write(
        std::path::Path::new(&workspace.worktree_path).join("uncommitted-repair.txt"),
        "not a clean committed repair\n",
    )
    .expect("make repair validation fail");

    let Json(response) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "The repair cannot be proven clean.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    .expect("blocked resurrection reports its validation failure");

    assert_eq!(response.status, "blocked");
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read repair attempt after failed resurrection")
        .expect("repair attempt remains current");
    assert_eq!(after.id, blocked.id);
    assert_eq!(after.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(state
        .app_state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&after.id)
        .await
        .expect("read continuation effect")
        .is_none());
    drop(root);
}

#[tokio::test]
async fn blocked_different_run_cannot_resurrect_the_current_generation() {
    let state = test_state();
    let (conversation_id, owner_run_id, _attempt, _root) =
        seed_current_attempt_with_valid_target(&state).await;
    let blocked = block_valid_current_attempt(&state, conversation_id, owner_run_id).await;
    let different_run = AgentRun::new(conversation_id);
    let different_run_id = different_run.id;
    state
        .app_state
        .agent_run_repo
        .create(different_run)
        .await
        .expect("seed nonowning repair run");

    let response = repair_completion_http_response(
        state.clone(),
        &conversation_id,
        completion_headers(conversation_id, different_run_id),
        CompleteAgentWorkspaceRepairRequest {
            summary: "A different run cannot settle this blocked repair.".to_string(),
            blocker: None,
            reported_fix_commit_sha: None,
            resolution: None,
        },
    )
    .await;
    let (status, outcome) = response_status(response).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(outcome.is_empty());
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read blocked repair attempt")
        .expect("blocked repair attempt remains current");
    assert_eq!(after.id, blocked.id);
    assert_eq!(after.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(after.updated_at, blocked.updated_at);
}

#[tokio::test]
async fn blocked_exact_run_with_blocker_keeps_the_canned_blocked_response() {
    let state = test_state();
    let (conversation_id, run_id, _attempt, _root) =
        seed_current_attempt_with_valid_target(&state).await;
    let blocked = block_valid_current_attempt(&state, conversation_id, run_id).await;

    let Json(response) = complete_agent_workspace_repair(
        State(state.clone()),
        Path(conversation_id.to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "A duplicate blocker must not resurrect the repair.".to_string(),
            blocker: Some("Still awaiting maintainer direction.".to_string()),
            reported_fix_commit_sha: None,
            resolution: None,
        }),
    )
    .await
    .expect("blocked duplicate responds idempotently");

    assert_eq!(response.status, "blocked");
    assert_eq!(
        response.message,
        "This repair generation is blocked. Retry repair from the workspace to start a new repair attempt."
    );
    let after = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read blocked repair attempt")
        .expect("blocked repair attempt remains current");
    assert_eq!(after.phase, AgentWorkspaceRepairPhase::Blocked);
    assert_eq!(after.updated_at, blocked.updated_at);
    assert!(after.target_lease_epoch.is_none());
}

#[tokio::test]
async fn workspace_response_projects_only_the_unsettled_maintenance_operation() {
    let state = test_state();
    let (conversation_id, _owner_run_id, attempt) = seed_current_attempt(&state).await;

    let Json(response) =
        get_agent_workspace_publish_status(State(state.clone()), Path(conversation_id.to_string()))
            .await
            .expect("workspace status response");
    let operation = response
        .workspace
        .maintenance_operation
        .expect("unsettled repair attempt must project into the workspace response");
    assert_eq!(operation.operation_id, attempt.id.to_string());
    assert_eq!(operation.generation, attempt.generation);
    assert_eq!(operation.stage.to_string(), "updating_base");
    assert_eq!(operation.status.to_string(), "active");
    assert!(operation.automatic_continuation);
}
