use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{MockGithubService, SubmittingPlanPrAgentClient};
use axum::{extract::Path, http::HeaderMap, Json};
use ralphx_lib::application::agent_conversation_workspace::{
    resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
    AgentConversationWorkspaceBaseSelection,
};
use ralphx_lib::application::agent_workspace_publish_recovery::{
    recover_agent_workspace_repair_after_terminal_run,
    recover_stale_agent_workspace_publish_repairs_for_state,
};
use ralphx_lib::application::agent_workspace_review::{
    apply_review_artifact_to_monitor, load_agent_workspace_review_context,
};
use ralphx_lib::application::{AppState, GitService};
use ralphx_lib::commands::{
    unified_chat_commands::{
        install_agent_workspace_repair_publish_continuation,
        publish_agent_conversation_workspace_for_app_state_with_repair_intent,
        set_agent_conversation_workspace_auto_publish_for_state,
        update_agent_conversation_workspace_from_base_for_app_state_with_caller,
        AgentConversationWorkspaceAutoPublishInput,
    },
    ExecutionState,
};
use ralphx_lib::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, AgentRunId, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    ArtifactId, ChatContextType, ChatConversation, ChatConversationId, GitTargetLeaseOwner,
    IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, PlanBranchId, Project, ProjectId,
};
use ralphx_lib::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, BindAgentWorkspaceRepairAttemptRun,
    CompleteAgentWorkspaceRepairEffect, CompleteAgentWorkspaceRepairEffectOutcome,
    StartOrJoinAgentWorkspaceRepairAttempt, StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use ralphx_lib::domain::review::ReviewSettings;
use ralphx_lib::domain::services::github_service::GithubServiceTrait;
use ralphx_lib::http_server::handlers::agent_workspaces::{
    clear_agent_workspace_repair_completion_continuation_gate_for_test,
    complete_agent_workspace_repair, complete_agent_workspace_review_run,
    set_agent_workspace_repair_completion_continuation_gate_for_test,
    CompleteAgentWorkspaceRepairRequest, CompleteAgentWorkspaceReviewRunRequest,
};
use ralphx_lib::http_server::types::HttpServerState;

fn git(repo: impl AsRef<std::path::Path>, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn advance_bare_remote_base(remote_path: &std::path::Path, parent: &str) -> String {
    let tree = git(remote_path, &["rev-parse", &format!("{parent}^{{tree}}")]);
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(remote_path)
        .args(["commit-tree", &tree, "-p", parent, "-m", "advance base"])
        .env("GIT_AUTHOR_NAME", "RalphX Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "RalphX Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("advance-base commit should spawn");
    assert!(
        output.status.success(),
        "advance-base commit failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let advanced_base = String::from_utf8_lossy(&output.stdout).trim().to_string();
    git(
        remote_path,
        &["update-ref", "refs/heads/main", &advanced_base],
    );
    advanced_base
}

fn make_http_state(app_state: AppState) -> HttpServerState {
    install_agent_workspace_repair_publish_continuation(&app_state);
    HttpServerState {
        app_state: Arc::new(app_state),
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
    }
}

async fn disable_workspace_review_gate(app_state: &AppState) {
    app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("disable workspace review policy for auto-publish fixture");
}

async fn seed_current_repair_attempt(
    app_state: &AppState,
    conversation_id: ChatConversationId,
) -> AgentRunId {
    let mut workspace = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load repair workspace")
        .expect("repair workspace exists");
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed accepted repair claim timestamp");
    let run = AgentRun::new(conversation_id);
    let run_id = run.id;
    app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed active repair run");
    let attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    let started = app_state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "repair completion test".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first repair generation must start");
    };
    let bound = app_state
        .agent_workspace_repair_repo
        .bind_repair_attempt_run(BindAgentWorkspaceRepairAttemptRun {
            attempt_id: started.id,
            generation: started.generation,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: started.updated_at,
            run_id,
            updated_at: chrono::Utc::now(),
        })
        .await
        .expect("bind repair run");
    assert!(matches!(
        bound,
        ralphx_lib::domain::repositories::AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    run_id
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

struct RewrittenRepairPublishFixture {
    _repo: tempfile::TempDir,
    _worktrees: tempfile::TempDir,
    project: Project,
    conversation_id: ChatConversationId,
    workspace_path: std::path::PathBuf,
    branch_name: String,
    base_sha: String,
    repaired_head: String,
    remote_path: std::path::PathBuf,
}

fn setup_rewritten_repair_publish_fixture(
    conversation_id: ChatConversationId,
    project_id: &str,
) -> RewrittenRepairPublishFixture {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    let remote_path = repo.path().join("origin.git");
    git(
        repo.path(),
        &["init", "--bare", remote_path.to_str().expect("remote path")],
    );
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    git(
        repo.path(),
        &[
            "remote",
            "add",
            "origin",
            remote_path.to_str().expect("remote path"),
        ],
    );
    git(repo.path(), &["push", "-u", "origin", "main"]);

    let mut project = Project::new(
        "Agent Workspace Repaired Publish".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(project_id.to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
    project.github_pr_enabled = true;

    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    let branch_name = "ralphx/test/agent-repaired-publish".to_string();
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            &branch_name,
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    git(&workspace_path, &["push", "-u", "origin", &branch_name]);
    std::fs::write(workspace_path.join("repair.txt"), "repair\n").expect("write repair file");
    git(&workspace_path, &["add", "repair.txt"]);
    git(&workspace_path, &["commit", "-m", "repair workspace"]);
    let repaired_head = git(&workspace_path, &["rev-parse", "HEAD"]);

    git(
        repo.path(),
        &["checkout", "-b", "remote-repair-head", "main"],
    );
    git(
        repo.path(),
        &["commit", "--allow-empty", "-m", "remote repair head"],
    );
    git(
        repo.path(),
        &[
            "push",
            "origin",
            &format!("remote-repair-head:refs/heads/{branch_name}"),
        ],
    );
    git(repo.path(), &["checkout", "main"]);
    git(
        repo.path(),
        &[
            "config",
            "remote.origin.pushurl",
            "git@github.com:ralphx-test/agent-workspace.git",
        ],
    );

    RewrittenRepairPublishFixture {
        _repo: repo,
        _worktrees: worktrees,
        project,
        conversation_id,
        workspace_path,
        branch_name,
        base_sha,
        repaired_head,
        remote_path,
    }
}

async fn seed_rewritten_repair_publish_workspace(
    app_state: &AppState,
    fixture: &RewrittenRepairPublishFixture,
) -> AgentRunId {
    app_state
        .project_repo
        .create(fixture.project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(fixture.project.id.clone());
    conversation.id = fixture.conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = fixture.project.id.as_str().to_string();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");
    let mut workspace = AgentConversationWorkspace::new(
        fixture.conversation_id,
        fixture.project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(fixture.base_sha.clone()),
        fixture.branch_name.clone(),
        fixture.workspace_path.to_string_lossy().to_string(),
    );
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let run_id = seed_current_repair_attempt(app_state, fixture.conversation_id).await;
    disable_workspace_review_gate(app_state).await;
    run_id
}

async fn checkpoint_current_repair_target_lease(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace_path: &std::path::Path,
    branch_name: &str,
    target_base_commit: &str,
) -> AgentWorkspaceRepairAttempt {
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load recovery repair attempt")
        .expect("recovery repair attempt is current");
    let identity = GitService::canonical_target_identity(workspace_path, branch_name)
        .await
        .expect("resolve canonical recovery target");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(current.id.as_str());
    let fencing_epoch = match state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner,
        })
        .await
        .expect("acquire durable recovery lease")
    {
        AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch }
        | AcquireGitTargetLeaseOutcome::AlreadyOwned { fencing_epoch } => fencing_epoch,
        outcome => panic!("recovery fixture target must be available, got {outcome:?}"),
    };
    let mut checkpointed = current.clone();
    checkpointed.target_base_commit = Some(target_base_commit.to_string());
    checkpointed.git_common_dir = Some(identity.git_common_dir().to_string_lossy().into_owned());
    checkpointed.target_ref = Some(identity.full_ref().to_string());
    checkpointed.target_identity_version = Some(1);
    checkpointed.target_lease_epoch = Some(fencing_epoch);
    checkpointed.phase = AgentWorkspaceRepairPhase::Repairing;
    checkpointed.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: checkpointed,
            expected_phase: current.phase,
            expected_updated_at: current.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint durable recovery lease")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected durable recovery lease checkpoint, got {outcome:?}"),
    }
}

async fn checkpoint_recovery_repair_target_lease(
    state: &AppState,
    fixture: &RewrittenRepairPublishFixture,
) -> AgentWorkspaceRepairAttempt {
    checkpoint_current_repair_target_lease(
        state,
        &fixture.conversation_id,
        &fixture.workspace_path,
        &fixture.branch_name,
        &fixture.base_sha,
    )
    .await
}

async fn seed_checkpointed_rewritten_repair_publish_workspace(
    app_state: &AppState,
    fixture: &RewrittenRepairPublishFixture,
) -> AgentRunId {
    let run_id = seed_rewritten_repair_publish_workspace(app_state, fixture).await;
    checkpoint_recovery_repair_target_lease(app_state, fixture).await;
    run_id
}

async fn auto_publish_push_boundary_diagnostics(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    mock_github: &MockGithubService,
    workspace_path: &std::path::Path,
    branch_name: &str,
) -> String {
    let attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await;
    let open_effect = match &attempt {
        Ok(Some(attempt)) => state
            .app_state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&attempt.id)
            .await
            .map(|effect| effect.map(|effect| (effect.kind, effect.status))),
        Ok(None) => Ok(None),
        Err(_) => Ok(None),
    };
    let lease = match GitService::canonical_target_identity(workspace_path, branch_name).await {
        Ok(identity) => state
            .app_state
            .branch_update_repo
            .get_target_lease(&identity)
            .await
            .map(|lease| {
                lease.map(|lease| {
                    (
                        lease.owner().clone(),
                        lease.fencing_epoch(),
                        lease.active_mutation().cloned(),
                        lease.is_released(),
                    )
                })
            }),
        Err(error) => Err(error),
    };
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await;
    let message_count = state
        .app_state
        .chat_message_repo
        .get_by_conversation(conversation_id)
        .await
        .map(|messages| messages.len());
    let exact_push_calls = *mock_github
        .push_branch_with_expected_remote_oid_lease_calls
        .lock()
        .expect("exact push counter lock");

    format!(
        "attempt={attempt:?}; expected_owner={}; checkpoint=(common_dir={:?}, ref={:?}, identity_version={:?}, epoch={:?}); lease={lease:?}; open_effect_count={}; open_effect={open_effect:?}; normal_push_calls={}; exact_push_calls={exact_push_calls}; pr_create_calls={}; workspace_pr={:?}; message_count={message_count:?}; queued_message_count={}",
        attempt
            .as_ref()
            .ok()
            .and_then(|attempt| attempt.as_ref())
            .map(|attempt| GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str()).owner_id)
            .unwrap_or_else(|| "<no-current-attempt>".to_string()),
        attempt
            .as_ref()
            .ok()
            .and_then(|attempt| attempt.as_ref())
            .and_then(|attempt| attempt.git_common_dir.as_ref()),
        attempt
            .as_ref()
            .ok()
            .and_then(|attempt| attempt.as_ref())
            .and_then(|attempt| attempt.target_ref.as_ref()),
        attempt
            .as_ref()
            .ok()
            .and_then(|attempt| attempt.as_ref())
            .and_then(|attempt| attempt.target_identity_version),
        attempt
            .as_ref()
            .ok()
            .and_then(|attempt| attempt.as_ref())
            .and_then(|attempt| attempt.target_lease_epoch),
        usize::from(matches!(open_effect, Ok(Some(_)))),
        mock_github.push_calls(),
        mock_github.create_calls(),
        workspace
            .as_ref()
            .ok()
            .and_then(|workspace| workspace.as_ref())
            .and_then(|workspace| workspace.publication_pr_number),
        state.app_state.message_queue.list_keys().len(),
    )
}

async fn park_current_repair_at_ready(
    state: &AppState,
    conversation_id: &ChatConversationId,
    target_base_commit: &str,
    repair_head_commit: &str,
) -> AgentWorkspaceRepairAttempt {
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load current repair attempt")
        .expect("current repair attempt should exist");
    let mut ready = current.clone();
    ready.phase = AgentWorkspaceRepairPhase::Ready;
    ready.target_base_commit = Some(target_base_commit.to_string());
    ready.repair_head_commit = Some(repair_head_commit.to_string());
    ready.summary = Some("Repair is ready for automatic publish continuation.".to_string());
    ready.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: ready,
            expected_phase: current.phase,
            expected_updated_at: current.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("park current repair at Ready")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(ready) => ready,
        outcome => panic!("expected Ready repair attempt, got {outcome:?}"),
    }
}

async fn backdate_current_ready_repair_attempt_for_recovery(
    state: &AppState,
    conversation_id: &ChatConversationId,
    continuation: Option<AgentWorkspaceRepairContinuation>,
    explicit_publish_requested: Option<bool>,
) -> AgentWorkspaceRepairAttempt {
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load Ready repair attempt")
        .expect("Ready repair attempt should remain current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);

    let mut ready = current.clone();
    if let Some(continuation) = continuation {
        ready.continuation = continuation;
    }
    if let Some(explicit_publish_requested) = explicit_publish_requested {
        ready.explicit_publish_requested = explicit_publish_requested;
    }
    ready.updated_at -= chrono::Duration::seconds(61);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: ready,
            expected_phase: current.phase,
            expected_updated_at: current.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("backdate Ready repair attempt for recovery")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(ready) => ready,
        outcome => panic!("expected backdated Ready repair attempt, got {outcome:?}"),
    }
}

async fn prepare_current_ready_repair_attempt_for_recovery(
    state: &AppState,
    conversation_id: &ChatConversationId,
    continuation: Option<AgentWorkspaceRepairContinuation>,
    explicit_publish_requested: Option<bool>,
) -> AgentWorkspaceRepairAttempt {
    let ready = backdate_current_ready_repair_attempt_for_recovery(
        state,
        conversation_id,
        continuation,
        explicit_publish_requested,
    )
    .await;
    if ready.git_common_dir.is_none()
        && ready.target_ref.is_none()
        && ready.target_identity_version.is_none()
        && ready.target_lease_epoch.is_none()
    {
        return ready;
    }

    recover_stale_agent_workspace_publish_repairs_for_state(state)
        .await
        .expect("release parked Ready repair target authority");
    backdate_current_ready_repair_attempt_for_recovery(state, conversation_id, None, None).await
}

async fn park_failed_ready_repair_redrive(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AgentWorkspaceRepairAttempt {
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load failed repair re-drive")
        .expect("failed repair re-drive remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Continuing);
    let mut effect = state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await
        .expect("load failed re-drive effect")
        .expect("failed re-drive keeps its effect receipt open");
    let expected_effect_updated_at = effect.updated_at;
    let expected_effect_status = effect.status;
    effect.status = ralphx_lib::domain::entities::AgentWorkspaceRepairEffectStatus::Failed;
    effect.last_error = Some("test fixture parks the failed re-drive".to_string());
    effect.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: current.updated_at,
            expected_effect_updated_at,
            expected_effect_status,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("close failed re-drive effect")
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(_) => {}
        outcome => panic!("expected failed re-drive effect completion, got {outcome:?}"),
    }

    let mut ready = current.clone();
    ready.phase = AgentWorkspaceRepairPhase::Ready;
    ready.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: ready,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_updated_at: current.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("park failed re-drive at Ready")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(ready) => ready,
        outcome => panic!("expected failed re-drive to park at Ready, got {outcome:?}"),
    }
}

async fn setup_durable_recovery_fixture(
    fixture: &RewrittenRepairPublishFixture,
) -> (HttpServerState, Arc<MockGithubService>, AgentRunId) {
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let workspace_repo = Arc::clone(&app_state.agent_conversation_workspace_repo);
    app_state =
        app_state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    let run_id = seed_checkpointed_rewritten_repair_publish_workspace(&app_state, fixture).await;
    (make_http_state(app_state), mock_github, run_id)
}

#[tokio::test]
async fn ready_publish_repair_stays_parked_when_auto_publish_disabled() {
    let conversation_id = ChatConversationId::from_string("63636363-6363-6363-6363-636363636363");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-ready-repair-auto-publish-disabled",
    );
    let (state, mock_github, _) = setup_durable_recovery_fixture(&fixture).await;
    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.auto_publish_enabled = false;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("disable auto publish");
    park_current_repair_at_ready(
        state.app_state.as_ref(),
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    prepare_current_ready_repair_attempt_for_recovery(
        state.app_state.as_ref(),
        &conversation_id,
        None,
        None,
    )
    .await;

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("recover parked repair"),
        0,
    );

    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read parked repair")
        .expect("unauthorized repair remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(current.settled_at.is_none());
    assert!(
        current
            .pending_reasons
            .iter()
            .all(|reason| !reason.starts_with("auto_retry_ready_repair:")),
        "unauthorized recovery must not spend a ready-retry streak"
    );
    assert_eq!(mock_github.push_calls(), 0);
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
    );
    assert_eq!(mock_github.create_calls(), 0);

    let expected_updated_at = current.updated_at;
    let mut exhausted = current.clone();
    exhausted
        .pending_reasons
        .push("auto_retry_ready_repair:3".to_string());
    exhausted.updated_at -= chrono::Duration::seconds(61);
    let exhausted = match state
        .app_state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: exhausted,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed an exhausted unauthorized Ready streak")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected exhausted Ready attempt, got {outcome:?}"),
    };
    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("recover exhausted unauthorized repair"),
        0,
    );
    let still_parked = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read exhausted unauthorized repair")
        .expect("exhausted unauthorized repair remains current");
    assert_eq!(still_parked.id, exhausted.id);
    assert_eq!(still_parked.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(still_parked.settled_at.is_none());
    assert!(still_parked.outcome.is_none());
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "an exhausted unauthorized attempt must not publish before settlement"
    );
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read publication events")
            .is_empty(),
        "unauthorized recovery must not record a publish effect"
    );
}

#[tokio::test]
async fn ready_publish_repair_redrives_when_auto_publish_enabled() {
    let conversation_id = ChatConversationId::from_string("64646464-6464-6464-6464-646464646464");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-ready-repair-auto-publish-enabled",
    );
    let (state, mock_github, _) = setup_durable_recovery_fixture(&fixture).await;
    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.auto_publish_enabled = true;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("enable auto publish");
    park_current_repair_at_ready(
        state.app_state.as_ref(),
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    prepare_current_ready_repair_attempt_for_recovery(
        state.app_state.as_ref(),
        &conversation_id,
        None,
        None,
    )
    .await;
    mock_github.will_fail_exact_lease_push("exercise authorized Ready re-drive");

    recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
        .await
        .expect("recover authorized repair");

    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read authorized repair")
        .expect("failed re-drive remains current for backoff");
    assert!(
        current
            .pending_reasons
            .iter()
            .any(|reason| reason == "auto_retry_ready_repair:1"),
        "authorized recovery records its bounded re-drive streak: {current:?}"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the live auto-publish toggle authorizes the recovery re-drive"
    );
}

#[tokio::test]
async fn toggle_authorized_redrive_does_not_grant_durable_publish_consent() {
    let conversation_id = ChatConversationId::from_string("67676767-6767-6767-6767-676767676767");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-toggle-redrive-publish-consent",
    );
    let (state, mock_github, _) = setup_durable_recovery_fixture(&fixture).await;
    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.auto_publish_enabled = true;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("enable auto publish");
    park_current_repair_at_ready(
        state.app_state.as_ref(),
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    prepare_current_ready_repair_attempt_for_recovery(
        state.app_state.as_ref(),
        &conversation_id,
        None,
        Some(false),
    )
    .await;
    mock_github.will_fail_exact_lease_push("exercise toggle-authorized Ready re-drive");

    recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
        .await
        .expect("recover toggle-authorized repair");

    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read toggle-authorized repair")
        .expect("failed re-drive remains current for backoff");
    assert!(
        !current.explicit_publish_requested,
        "automation authority must not become durable user consent"
    );
    let ready_retry_markers = current
        .pending_reasons
        .iter()
        .filter(|reason| reason.starts_with("auto_retry_ready_repair:"))
        .count();
    park_failed_ready_repair_redrive(state.app_state.as_ref(), &conversation_id).await;

    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("reload workspace")
        .expect("workspace remains current");
    workspace.auto_publish_enabled = false;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("disable auto publish");
    backdate_current_ready_repair_attempt_for_recovery(
        state.app_state.as_ref(),
        &conversation_id,
        None,
        None,
    )
    .await;
    *mock_github
        .push_branch_with_expected_remote_oid_lease_calls
        .lock()
        .expect("reset exact push counter") = 0;

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("recover after disabling auto publish"),
        0,
    );

    let parked = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read parked repair")
        .expect("repair stays parked after toggle off");
    assert_eq!(parked.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(parked.settled_at.is_none());
    assert_eq!(
        parked
            .pending_reasons
            .iter()
            .filter(|reason| reason.starts_with("auto_retry_ready_repair:"))
            .count(),
        ready_retry_markers,
        "an unauthorized sweep must not spend another ready-retry marker"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "turning Auto Publish off must revoke timer re-drive authority"
    );
}

#[tokio::test]
async fn resume_pr_supervision_redrive_does_not_grant_durable_publish_consent() {
    let conversation_id = ChatConversationId::from_string("68686868-6868-6868-6868-686868686868");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-supervision-redrive-publish-consent",
    );
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.auto_publish_enabled = false;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("disable auto publish");
    park_current_repair_at_ready(
        state.app_state.as_ref(),
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    prepare_current_ready_repair_attempt_for_recovery(
        state.app_state.as_ref(),
        &conversation_id,
        Some(AgentWorkspaceRepairContinuation::ResumePrSupervision),
        Some(false),
    )
    .await;
    mock_github.will_fail_exact_lease_push("exercise supervision-authorized Ready re-drive");

    recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
        .await
        .expect("recover supervision-authorized repair");

    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read supervision-authorized repair")
        .expect("failed supervision re-drive remains current");
    assert_eq!(
        current.continuation,
        AgentWorkspaceRepairContinuation::ResumePrSupervision
    );
    assert!(
        !current.explicit_publish_requested,
        "PR supervision authority must not become durable user consent"
    );
    park_failed_ready_repair_redrive(state.app_state.as_ref(), &conversation_id).await;

    checkpoint_current_repair_target_lease(
        state.app_state.as_ref(),
        &conversation_id,
        &fixture.workspace_path,
        &fixture.branch_name,
        &fixture.base_sha,
    )
    .await;
    let _ = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Completed the supervision repair after Auto Publish was disabled".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("toggle-off completion boundary should accept the clean repair");

    let parked = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read completed supervision repair")
        .expect("toggle-off completion keeps the repair current");
    assert_eq!(parked.phase, AgentWorkspaceRepairPhase::Ready);
    assert!(!parked.explicit_publish_requested);
    assert!(parked.settled_at.is_none());
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the completion boundary must park instead of publishing again"
    );
}

#[tokio::test]
async fn ready_publish_repair_redrives_when_user_consent_persisted() {
    let conversation_id = ChatConversationId::from_string("65656565-6565-6565-6565-656565656565");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-ready-repair-persisted-publish-consent",
    );
    let (state, mock_github, _) = setup_durable_recovery_fixture(&fixture).await;
    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.auto_publish_enabled = false;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("disable auto publish");
    park_current_repair_at_ready(
        state.app_state.as_ref(),
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    prepare_current_ready_repair_attempt_for_recovery(
        state.app_state.as_ref(),
        &conversation_id,
        None,
        Some(true),
    )
    .await;
    mock_github.will_fail_exact_lease_push("exercise consent-authorized Ready re-drive");

    recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
        .await
        .expect("recover user-consented repair");

    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read consent-authorized repair")
        .expect("failed re-drive remains current for backoff");
    assert!(current.explicit_publish_requested);
    assert!(
        current
            .pending_reasons
            .iter()
            .any(|reason| reason == "auto_retry_ready_repair:1"),
        "consent-authorized recovery records its bounded re-drive streak: {current:?}"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "persisted user consent authorizes recovery while the live toggle is off"
    );
}

#[tokio::test]
async fn ready_update_only_repair_is_never_upgraded_to_publish_by_recovery() {
    let conversation_id = ChatConversationId::from_string("66666666-6666-6666-6666-666666666666");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-ready-update-only-repair-recovery",
    );
    let (state, mock_github, _) = setup_durable_recovery_fixture(&fixture).await;
    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.auto_publish_enabled = true;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("enable auto publish");
    park_current_repair_at_ready(
        state.app_state.as_ref(),
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    prepare_current_ready_repair_attempt_for_recovery(
        state.app_state.as_ref(),
        &conversation_id,
        Some(AgentWorkspaceRepairContinuation::UpdateOnly),
        Some(true),
    )
    .await;

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("recover update-only repair"),
        0,
    );

    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read update-only repair")
        .expect("update-only repair remains current");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(
        current.continuation,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "recovery must not upgrade an update-only repair into a publish continuation"
    );
    assert!(
        current
            .pending_reasons
            .iter()
            .all(|reason| !reason.starts_with("auto_retry_ready_repair:")),
        "update-only recovery must not spend a publish retry streak"
    );
    assert_eq!(mock_github.push_calls(), 0);
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
    );
    assert_eq!(mock_github.create_calls(), 0);
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read publication events")
            .is_empty(),
        "update-only recovery must not record a publish effect"
    );
}

async fn assert_recovery_blocked_without_effects(
    state: &HttpServerState,
    mock_github: &MockGithubService,
    conversation_id: &ChatConversationId,
) {
    let attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
        .expect("load blocked recovery attempt")
        .expect("blocked recovery attempt remains current");
    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        attempt.blocker.is_some(),
        "blocked recovery keeps actionable truth"
    );
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0,
        "blocked recovery must not enter the normal publisher"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "blocked recovery must not push the repair branch"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "blocked recovery must not create or reconcile a pull request"
    );
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(conversation_id)
            .await
            .expect("read recovery events")
            .is_empty(),
        "blocked recovery must not redispatch a repair or start a review"
    );
    assert_eq!(
        state
            .app_state
            .agent_run_repo
            .get_by_conversation(conversation_id)
            .await
            .expect("read recovery runs")
            .len(),
        1,
        "blocked recovery must not dispatch a duplicate repair run"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(conversation_id)
            .await
            .expect("read recovery messages")
            .is_empty(),
        "blocked recovery must not emit a duplicate repair message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "blocked recovery must not enqueue a duplicate repair message"
    );
}

#[tokio::test]
async fn terminal_then_startup_recovery_continues_a_clean_durable_repair_once() {
    let conversation_id = ChatConversationId::from_string("51515151-5151-5151-5151-515151515151");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-terminal-clean-repair-recovery",
    );
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    state
        .app_state
        .agent_run_repo
        .fail(&run_id, "repair process stopped after committing")
        .await
        .expect("terminalize repair run");

    let push_started = Arc::new(tokio::sync::Notify::new());
    *mock_github
        .push_branch_with_expected_remote_oid_lease_delay_ms
        .lock()
        .expect("exact push delay lock") = 1_000;
    *mock_github
        .push_branch_with_expected_remote_oid_lease_started
        .lock()
        .expect("exact push notification lock") = Some(Arc::clone(&push_started));
    let terminal_state = state.app_state.clone();
    let terminal_conversation_id = conversation_id;
    let terminal_run_id = run_id;
    let mut terminal_recovery = tokio::spawn(async move {
        recover_agent_workspace_repair_after_terminal_run(
            terminal_state.as_ref(),
            &terminal_conversation_id,
            &terminal_run_id,
        )
        .await
    });
    tokio::select! {
        _ = push_started.notified() => {}
        terminal_outcome = &mut terminal_recovery => {
            let diagnostics = auto_publish_push_boundary_diagnostics(
                &state,
                &conversation_id,
                mock_github.as_ref(),
                &fixture.workspace_path,
                &fixture.branch_name,
            )
            .await;
            panic!(
                "terminal recovery returned before the exact push boundary; recovery_outcome={terminal_outcome:?}; {diagnostics}"
            );
        }
        _ = tokio::time::sleep(Duration::from_secs(5)) => {
            let diagnostics = auto_publish_push_boundary_diagnostics(
                &state,
                &conversation_id,
                mock_github.as_ref(),
                &fixture.workspace_path,
                &fixture.branch_name,
            )
            .await;
            panic!(
                "terminal recovery did not reach the exact push boundary within five seconds; {diagnostics}"
            );
        }
    }
    let startup_state = state.app_state.clone();
    let startup_recovery = tokio::spawn(async move {
        recover_stale_agent_workspace_publish_repairs_for_state(startup_state.as_ref()).await
    });
    let source_refspec = format!(
        "refs/heads/{}:refs/ralphx-test/recovery-source",
        fixture.branch_name
    );
    git(
        &fixture.remote_path,
        &[
            "fetch",
            fixture.workspace_path.to_str().expect("workspace path"),
            &source_refspec,
        ],
    );
    git(
        &fixture.remote_path,
        &[
            "update-ref",
            &format!("refs/heads/{}", fixture.branch_name),
            &fixture.repaired_head,
        ],
    );
    let recovered = tokio::time::timeout(Duration::from_secs(5), terminal_recovery)
        .await
        .unwrap_or_else(|_| panic!("terminal recovery did not settle after the remote update"))
        .expect("terminal recovery task joins")
        .expect("recover terminal committed repair");
    let startup_recovered = match tokio::time::timeout(Duration::from_secs(5), startup_recovery)
        .await
    {
        Ok(startup_recovery) => startup_recovery
            .expect("startup recovery task joins")
            .expect("startup recovery should join safely"),
        Err(_) => {
            let diagnostics = auto_publish_push_boundary_diagnostics(
                &state,
                &conversation_id,
                mock_github.as_ref(),
                &fixture.workspace_path,
                &fixture.branch_name,
            )
            .await;
            panic!(
                "startup recovery did not settle while the terminal continuation owned the push; {diagnostics}"
            );
        }
    };
    let final_recovery =
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("final recovery should observe the single durable continuation");
    assert!(
        recovered
            || startup_recovered > 0
            || final_recovery > 0
            || *mock_github
                .push_branch_with_expected_remote_oid_lease_calls
                .lock()
                .expect("exact push counter after concurrent recovery")
                > 0,
        "one of the scheduled, terminal, or startup recovery owners must continue the clean repair"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "workspace, terminal, and startup recovery must share one repair-owned push"
    );
    assert_eq!(
        mock_github.create_calls(),
        1,
        "terminal recovery creates one PR handoff"
    );
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read terminal recovery events")
            .iter()
            .filter(|event| event.step == "published")
            .count(),
        1,
        "terminal recovery emits one publication event"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read concurrent recovery messages")
            .is_empty(),
        "workspace, terminal, and startup recovery must not emit a duplicate worker message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "workspace, terminal, and startup recovery must not enqueue a duplicate worker message"
    );

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("startup replay must be idempotent"),
        0,
        "startup must not revive the settled repair generation"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter after replay"),
        1,
        "startup replay must not duplicate the repair push"
    );
    assert_eq!(
        mock_github.create_calls(),
        1,
        "startup replay must not duplicate PR creation or review continuation"
    );
}

#[tokio::test]
async fn terminal_recovery_reports_a_failed_clean_repair_continuation_as_pending_then_reconciles_once(
) {
    let conversation_id = ChatConversationId::from_string("58585858-5858-5858-5858-585858585858");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-terminal-continuation-failure-recovery",
    );
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    mock_github.will_fail_exact_lease_push("simulated exact-lease push interruption");
    state
        .app_state
        .agent_run_repo
        .fail(&run_id, "repair process stopped after committing")
        .await
        .expect("terminalize repair run");

    assert!(
        !recover_agent_workspace_repair_after_terminal_run(
            state.app_state.as_ref(),
            &conversation_id,
            &run_id,
        )
        .await
        .expect("recover interrupted clean repair"),
        "a continuation error must not report the clean repair as continued"
    );

    let pending = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load pending repair attempt")
        .expect("continuation failure must retain the current generation");
    assert_eq!(pending.phase, AgentWorkspaceRepairPhase::Continuing);
    assert!(
        pending
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("pending reconciliation")),
        "the durable attempt must record why recovery did not finish"
    );
    assert!(
        pending.blocker.is_none(),
        "an uncertain push receipt must remain recoverable instead of being falsely blocked"
    );
    assert!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&pending.id)
            .await
            .expect("read pending push effect")
            .is_some(),
        "the uncertain external effect must retain durable recovery authority"
    );
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load compatibility projection")
        .expect("workspace remains available");
    assert_eq!(
        workspace.publication_push_status.as_deref(),
        Some("refreshed")
    );
    assert_eq!(
        workspace.pr_supervision_status.as_deref(),
        Some("publishing")
    );
    assert!(
        workspace
            .pr_supervision_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("pending reconciliation")),
        "the compatibility projection must not claim successful continuation"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the failed recovery issues one exact-lease push attempt"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "failure must not create a PR"
    );
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read recovery events")
            .iter()
            .all(|event| event.step != "published"),
        "failure must not record a successful publication event"
    );

    let source_refspec = format!(
        "refs/heads/{}:refs/ralphx-test/continuation-failure-source",
        fixture.branch_name
    );
    git(
        &fixture.remote_path,
        &[
            "fetch",
            fixture.workspace_path.to_str().expect("workspace path"),
            &source_refspec,
        ],
    );
    git(
        &fixture.remote_path,
        &[
            "update-ref",
            &format!("refs/heads/{}", fixture.branch_name),
            &fixture.repaired_head,
        ],
    );
    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("restart recovery reconciles the observed push"),
        1,
        "restart recovery must converge the exact pending generation"
    );
    assert!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read settled repair")
            .is_none(),
        "the reconciled generation must settle after its durable handoff"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter after reconciliation"),
        1,
        "observed postcondition reconciliation must not issue a duplicate push"
    );
    assert_eq!(
        mock_github.create_calls(),
        1,
        "reconciliation creates one PR handoff"
    );

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("settled replay is idempotent"),
        0
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter after replay"),
        1,
        "replay must not duplicate the reconciled push"
    );
    assert_eq!(
        mock_github.create_calls(),
        1,
        "replay must not create another PR"
    );
}

#[tokio::test]
async fn terminal_recovery_continues_a_clean_repair_when_its_reserved_run_row_is_missing() {
    let conversation_id = ChatConversationId::from_string("57575757-5757-5757-5757-575757575757");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-missing-run-repair-recovery",
    );
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    let mut workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    workspace.auto_publish_enabled = false;
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("disable auto publish for missing-run recovery");
    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load repair attempt")
        .expect("repair attempt is current");
    let missing_run_id = AgentRunId::from_string("57575757-5757-5757-5757-575757575758");
    let mut missing_run_attempt = current.clone();
    missing_run_attempt.reserved_agent_run_id = Some(missing_run_id);
    missing_run_attempt.updated_at += chrono::Duration::microseconds(1);
    let missing_run_attempt = match state
        .app_state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: missing_run_attempt,
            expected_phase: current.phase,
            expected_updated_at: current.updated_at,
            next_phase: current.phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint missing run reservation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected missing-run checkpoint, got {outcome:?}"),
    };

    state
        .app_state
        .agent_run_repo
        .delete(&run_id)
        .await
        .expect("remove the superseded seeded repair run");
    recover_agent_workspace_repair_after_terminal_run(
        state.app_state.as_ref(),
        &conversation_id,
        &missing_run_id,
    )
    .await
    .expect("recover missing reserved run");
    let recovered = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load recovered repair")
        .expect("ready repair remains current");
    assert_eq!(recovered.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(
        recovered.repair_head_commit.as_deref(),
        Some(fixture.repaired_head.as_str())
    );
    assert_eq!(recovered.id, missing_run_attempt.id);
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "Auto Publish off must preserve the recovered repair without a push"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "missing-run recovery must not create a PR before an explicit publish"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read messages")
            .is_empty(),
        "missing-run recovery must not dispatch another repair agent"
    );
}

#[tokio::test]
async fn startup_recovery_blocks_a_dirty_durable_repair_without_effects() {
    let conversation_id = ChatConversationId::from_string("52525252-5252-5252-5252-525252525252");
    let fixture =
        setup_rewritten_repair_publish_fixture(conversation_id, "project-dirty-repair-recovery");
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    std::fs::write(
        fixture.workspace_path.join("dirty.txt"),
        "interrupted repair\n",
    )
    .expect("make repair workspace dirty");
    state
        .app_state
        .agent_run_repo
        .fail(&run_id, "repair stopped with uncommitted changes")
        .await
        .expect("terminalize dirty repair run");

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("recover dirty repair"),
        1
    );
    assert_recovery_blocked_without_effects(&state, mock_github.as_ref(), &conversation_id).await;
}

#[tokio::test]
async fn terminal_recovery_blocks_a_conflicted_durable_repair_without_effects() {
    let conversation_id = ChatConversationId::from_string("53535353-5353-5353-5353-535353535353");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-conflicted-repair-recovery",
    );
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    std::fs::write(fixture.workspace_path.join("README.md"), "repair version\n")
        .expect("write repair README");
    git(&fixture.workspace_path, &["add", "README.md"]);
    git(&fixture.workspace_path, &["commit", "-m", "repair README"]);
    std::fs::write(fixture._repo.path().join("README.md"), "target version\n")
        .expect("write target README");
    git(fixture._repo.path(), &["add", "README.md"]);
    git(fixture._repo.path(), &["commit", "-m", "target README"]);
    let merge = Command::new("git")
        .args(["merge", "main"])
        .current_dir(&fixture.workspace_path)
        .output()
        .expect("start conflicting merge");
    assert!(
        !merge.status.success(),
        "fixture merge must leave a conflict"
    );
    state
        .app_state
        .agent_run_repo
        .fail(&run_id, "repair stopped in merge conflict")
        .await
        .expect("terminalize conflicted repair run");

    assert!(recover_agent_workspace_repair_after_terminal_run(
        state.app_state.as_ref(),
        &conversation_id,
        &run_id,
    )
    .await
    .expect("recover conflicted repair"));
    assert_recovery_blocked_without_effects(&state, mock_github.as_ref(), &conversation_id).await;
}

#[tokio::test]
async fn startup_recovery_blocks_a_base_mismatch_without_effects() {
    let conversation_id = ChatConversationId::from_string("54545454-5454-5454-5454-545454545454");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-base-mismatch-repair-recovery",
    );
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    advance_bare_remote_base(&fixture.remote_path, &fixture.base_sha);
    state
        .app_state
        .agent_run_repo
        .fail(&run_id, "repair stopped behind the target base")
        .await
        .expect("terminalize base-mismatched repair run");

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("recover base-mismatched repair"),
        1
    );
    assert_recovery_blocked_without_effects(&state, mock_github.as_ref(), &conversation_id).await;
}

#[tokio::test]
async fn terminal_recovery_blocks_stale_lease_authority_without_effects() {
    let conversation_id = ChatConversationId::from_string("56565656-5656-5656-5656-565656565656");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-stale-authority-repair-recovery",
    );
    let (state, mock_github, run_id) = setup_durable_recovery_fixture(&fixture).await;
    let attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load checkpointed repair")
        .expect("checkpointed repair remains current");
    let identity =
        GitService::canonical_target_identity(&fixture.workspace_path, &fixture.branch_name)
            .await
            .expect("resolve canonical target");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    state
        .app_state
        .branch_update_repo
        .release_target_lease(
            &identity,
            &owner,
            attempt
                .target_lease_epoch
                .expect("checkpointed lease epoch"),
        )
        .await
        .expect("release original repair lease");
    assert!(matches!(
        state
            .app_state
            .branch_update_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity,
                owner: GitTargetLeaseOwner::branch_update("newer-owner", "newer-update"),
            })
            .await
            .expect("acquire newer target authority"),
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));
    state
        .app_state
        .agent_run_repo
        .fail(&run_id, "repair stopped after losing authority")
        .await
        .expect("terminalize stale-authority run");

    assert!(recover_agent_workspace_repair_after_terminal_run(
        state.app_state.as_ref(),
        &conversation_id,
        &run_id,
    )
    .await
    .expect("recover stale-authority repair"));
    assert_recovery_blocked_without_effects(&state, mock_github.as_ref(), &conversation_id).await;
}

#[tokio::test]
async fn complete_repair_hands_off_auto_publish_to_durable_continuation() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");

    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let mut project = Project::new(
        "Agent Workspace Auto Publish".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-auto-publish".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());

    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    let branch_name = "ralphx/test/agent-auto-publish";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(workspace_path.join("repair.txt"), "repair\n").expect("write repair file");
    git(&workspace_path, &["add", "repair.txt"]);
    git(&workspace_path, &["commit", "-m", "repair workspace"]);
    let _repair_sha = git(&workspace_path, &["rev-parse", "HEAD"]);

    let app_state = AppState::new_test();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = project.id.as_str().to_string();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha.clone()),
        branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let run_id = seed_current_repair_attempt(&app_state, conversation_id).await;
    checkpoint_current_repair_target_lease(
        &app_state,
        &conversation_id,
        &workspace_path,
        branch_name,
        &base_sha,
    )
    .await;
    disable_workspace_review_gate(&app_state).await;

    let state = make_http_state(app_state);
    let response = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Resolved the stale base repair".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("repair completion should succeed")
    .0;

    assert_eq!(response.status, "accepted");
    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read repair attempt")
        .expect("repair attempt remains current");
    assert_eq!(
        current.phase,
        AgentWorkspaceRepairPhase::Blocked,
        "the repair completion is accepted, while a local-only project fails closed before PR continuation",
    );
    assert!(
        current
            .blocker
            .as_deref()
            .is_some_and(|blocker| blocker.contains("GitHub integration is unavailable")),
        "the durable attempt must retain the continuation blocker instead of claiming a pending PR handoff",
    );
    let event_count = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read events after accepted completion")
        .len();

    let duplicate = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Duplicate stale completion".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("duplicate completion is idempotent")
    .0;
    assert_eq!(duplicate.status, "blocked");
    let after_duplicate = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read repair attempt after duplicate")
        .expect("repair attempt remains current after duplicate");
    assert_eq!(after_duplicate.id, current.id);
    assert_eq!(after_duplicate.phase, current.phase);
    assert_eq!(after_duplicate.updated_at, current.updated_at);
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read events after duplicate completion")
            .len(),
        event_count,
        "duplicate completion must not append events or resume twice"
    );
}

#[tokio::test]
async fn ready_repair_publish_uses_durable_continuation_not_normal_publisher() {
    let conversation_id = ChatConversationId::from_string("55555555-5555-5555-5555-555555555555");
    let fixture =
        setup_rewritten_repair_publish_fixture(conversation_id, "project-ready-repair-publish");
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let workspace_repo = Arc::clone(&app_state.agent_conversation_workspace_repo);
    app_state =
        app_state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    let run_id = seed_checkpointed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    let mut workspace = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load seeded workspace")
        .expect("seeded workspace exists");
    workspace.auto_publish_enabled = false;
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("disable auto publish to create a Ready continuation");
    let state = make_http_state(app_state);

    let completed = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Resolved the manual publish repair".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("repair completion should reach the Ready continuation")
    .0;
    assert_eq!(completed.status, "accepted");
    assert_eq!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read ready repair attempt")
            .expect("repair attempt remains current")
            .phase,
        AgentWorkspaceRepairPhase::Ready
    );

    let push_started = Arc::new(tokio::sync::Notify::new());
    *mock_github
        .push_branch_with_expected_remote_oid_lease_delay_ms
        .lock()
        .expect("exact push delay lock") = 50;
    *mock_github
        .push_branch_with_expected_remote_oid_lease_started
        .lock()
        .expect("exact push notification lock") = Some(Arc::clone(&push_started));
    let first_state = state.clone();
    let first_conversation_id = conversation_id;
    let first = tokio::spawn(async move {
        publish_agent_conversation_workspace_for_app_state_with_repair_intent(
            first_state.app_state.as_ref(),
            &first_state.execution_state,
            first_conversation_id,
            true,
            true,
        )
        .await
    });
    push_started.notified().await;
    let source_refspec = format!(
        "refs/heads/{}:refs/ralphx-test/ready-repair-source",
        fixture.branch_name
    );
    git(
        &fixture.remote_path,
        &[
            "fetch",
            fixture.workspace_path.to_str().expect("workspace path"),
            &source_refspec,
        ],
    );
    git(
        &fixture.remote_path,
        &[
            "update-ref",
            &format!("refs/heads/{}", fixture.branch_name),
            &fixture.repaired_head,
        ],
    );
    let duplicate = publish_agent_conversation_workspace_for_app_state_with_repair_intent(
        state.app_state.as_ref(),
        &state.execution_state,
        conversation_id,
        true,
        true,
    )
    .await;
    assert!(
        duplicate.is_err(),
        "a concurrent Ready continuation must fail closed instead of entering the normal publisher"
    );
    let published = first
        .await
        .expect("owner continuation task joins")
        .expect("manual publish should resume the durable repair continuation");

    assert!(published.pushed);
    assert_eq!(published.pr_number, Some(1));
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0,
        "a Ready repair must never fall through to the normal publisher"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the durable repair publisher must own the repaired branch push"
    );
    assert_eq!(mock_github.create_calls(), 1);
    assert!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read settled repair attempt")
            .is_none(),
        "the attempt settles only after the durable PR-monitor handoff"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read duplicate publish messages")
            .is_empty(),
        "the rejected duplicate must not emit a message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "the rejected duplicate must not enqueue a message"
    );
}

#[tokio::test]
async fn auto_publish_initial_opt_in_resumes_ready_publish_repair_once() {
    let conversation_id = ChatConversationId::from_string("56565656-5656-5656-5656-565656565656");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-auto-publish-initial-ready-repair",
    );
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let workspace_repo = Arc::clone(&app_state.agent_conversation_workspace_repo);
    app_state =
        app_state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    seed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    let ready = park_current_repair_at_ready(
        &app_state,
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    assert_eq!(ready.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(
        ready.continuation,
        AgentWorkspaceRepairContinuation::Publish,
        "only a persisted Publish continuation may resume automatically"
    );
    let state = make_http_state(app_state);

    let push_started = Arc::new(tokio::sync::Notify::new());
    *mock_github
        .push_branch_with_expected_remote_oid_lease_delay_ms
        .lock()
        .expect("exact push delay lock") = 50;
    *mock_github
        .push_branch_with_expected_remote_oid_lease_started
        .lock()
        .expect("exact push notification lock") = Some(Arc::clone(&push_started));
    let first_state = state.clone();
    let first_conversation_id = conversation_id;
    let first = tokio::spawn(async move {
        set_agent_conversation_workspace_auto_publish_for_state(
            first_conversation_id.as_str().to_string(),
            AgentConversationWorkspaceAutoPublishInput {
                auto_publish_enabled: true,
            },
            first_state.app_state.as_ref(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(1), push_started.notified())
        .await
        .expect("initial opt-in should reach the repair-owned push boundary");
    let duplicate_while_publishing = set_agent_conversation_workspace_auto_publish_for_state(
        conversation_id.as_str().to_string(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        state.app_state.as_ref(),
    )
    .await
    .expect("repeated enable must join the current durable continuation");
    assert!(duplicate_while_publishing.auto_publish_initial_pr_enabled);
    assert!(
        duplicate_while_publishing.maintenance_operation.is_some(),
        "the duplicate response must report the in-flight durable operation"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter while duplicate returns"),
        1,
        "repeated enable must not create a second exact push while the first is in flight"
    );
    let source_refspec = format!(
        "refs/heads/{}:refs/ralphx-test/auto-publish-initial-source",
        fixture.branch_name
    );
    git(
        &fixture.remote_path,
        &[
            "fetch",
            fixture.workspace_path.to_str().expect("workspace path"),
            &source_refspec,
        ],
    );
    git(
        &fixture.remote_path,
        &[
            "update-ref",
            &format!("refs/heads/{}", fixture.branch_name),
            &fixture.repaired_head,
        ],
    );
    let response = first
        .await
        .expect("auto-publish command task joins")
        .expect("initial Auto Publish opt-in should resume the durable repair");

    assert!(response.auto_publish_initial_pr_enabled);
    assert_eq!(
        response.publication_pr_number,
        Some(1),
        "Auto Publish response must reflect the durable handoff: {response:?}"
    );
    assert!(
        response.maintenance_operation.is_none(),
        "a completed synchronous continuation must not report a stale Ready operation"
    );
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0,
        "automatic Ready resume must stay on the repair-owned push path"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the Ready generation must issue one exact lease-protected push"
    );
    assert_eq!(mock_github.create_calls(), 1);

    let replay = set_agent_conversation_workspace_auto_publish_for_state(
        conversation_id.as_str().to_string(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        state.app_state.as_ref(),
    )
    .await
    .expect("already-enabled Auto Publish should remain an idempotent no-op");
    assert_eq!(replay.publication_pr_number, Some(1));
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter after replay"),
        1,
        "repeated enable must not create another repair push"
    );
    assert_eq!(
        mock_github.create_calls(),
        1,
        "repeated enable must not create another pull request handoff"
    );
}

#[tokio::test]
async fn auto_publish_enable_fails_closed_when_ready_repair_target_lease_is_foreign() {
    let conversation_id = ChatConversationId::from_string("57575757-5757-5757-5757-575757575757");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-auto-publish-foreign-ready-repair",
    );
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let workspace_repo = Arc::clone(&app_state.agent_conversation_workspace_repo);
    app_state =
        app_state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    seed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    let ready = park_current_repair_at_ready(
        &app_state,
        &conversation_id,
        &fixture.base_sha,
        &fixture.repaired_head,
    )
    .await;
    let identity =
        GitService::canonical_target_identity(&fixture.workspace_path, &fixture.branch_name)
            .await
            .expect("resolve canonical repair target");
    assert!(matches!(
        app_state
            .branch_update_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity,
                owner: GitTargetLeaseOwner::branch_update("foreign-owner", "foreign-update"),
            })
            .await
            .expect("foreign owner should acquire the target lease"),
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));
    let state = make_http_state(app_state);

    let response = set_agent_conversation_workspace_auto_publish_for_state(
        conversation_id.as_str().to_string(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        state.app_state.as_ref(),
    )
    .await
    .expect("preference should persist while the foreign lease fences continuation");

    assert!(response.auto_publish_initial_pr_enabled);
    assert_eq!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read fenced repair")
            .expect("fenced repair remains current")
            .id,
        ready.id
    );
    assert_eq!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read fenced repair phase")
            .expect("fenced repair remains current")
            .phase,
        AgentWorkspaceRepairPhase::Ready,
        "a foreign lease must leave the durable attempt at its paused boundary"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "foreign lease rejection must occur before a repair-owned push"
    );
    assert_eq!(mock_github.create_calls(), 0);
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read repair messages")
            .is_empty(),
        "foreign lease rejection must not emit a duplicate repair message"
    );
    assert!(state.app_state.message_queue.list_keys().is_empty());
}

#[tokio::test]
async fn passed_workspace_review_resumes_the_current_durable_repair_generation() {
    // Keep this full production-path scenario off libtest's platform-sized thread stack.
    Box::pin(passed_workspace_review_resumes_the_current_durable_repair_generation_body()).await;
}

async fn passed_workspace_review_resumes_the_current_durable_repair_generation_body() {
    let conversation_id = ChatConversationId::from_string("66666666-6666-6666-6666-666666666666");
    let fixture =
        setup_rewritten_repair_publish_fixture(conversation_id, "project-review-repair-publish");
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let workspace_repo = Arc::clone(&app_state.agent_conversation_workspace_repo);
    app_state =
        app_state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    let run_id = seed_checkpointed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: true,
            ..ReviewSettings::default()
        })
        .await
        .expect("require workspace review before the repair continuation");
    let workspace = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace for the existing Workspace Review")
        .expect("workspace exists");
    let mut monitor = load_agent_workspace_review_context(&app_state, &workspace)
        .await
        .expect("load Workspace Review target")
        .monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.last_run_id = Some("current-durable-reviewer".to_string());
    app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("persist the existing Workspace Review monitor");
    let state = make_http_state(app_state);

    let completed = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Resolved the repair awaiting Workspace Review".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("repair completion should hand off to Workspace Review")
    .0;
    let handoff_attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read attempted Workspace Review handoff");
    assert_eq!(
        completed.status, "accepted",
        "Workspace Review startup unexpectedly blocked the durable repair: {handoff_attempt:?}"
    );
    assert_eq!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read awaiting-review attempt")
            .expect("repair attempt remains current")
            .phase,
        AgentWorkspaceRepairPhase::AwaitingReview
    );

    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace for Workspace Review")
        .expect("workspace exists");
    let review_context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .expect("load review context");
    let target = review_context.target.expect("review target exists");
    let mut monitor = review_context.monitor;
    assert_eq!(
        monitor.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing,
        "the durable AwaitingReview handoff must retain the existing Workspace Reviewer"
    );
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Reviewing,
        "the existing reviewer must retain the repair generation at the persisted review gate"
    );
    let reviewer_run_id = monitor
        .last_run_id
        .clone()
        .expect("the started reviewer must reserve one runtime run");
    let reviewer_events_before = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read reviewer handoff events")
        .into_iter()
        .filter(|event| event.step == "workspace_review" && event.status == "reviewing")
        .count();
    assert_eq!(
        reviewer_events_before, 0,
        "the repair boundary must reuse the already-durable reviewer without a second start"
    );

    let duplicate = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Duplicate repair completion must not start another reviewer".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("duplicate repair completion is idempotent")
    .0;
    assert_eq!(duplicate.status, "already_completed");
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read reviewer events after duplicate completion")
            .into_iter()
            .filter(|event| event.step == "workspace_review" && event.status == "reviewing")
            .count(),
        reviewer_events_before,
        "duplicate repair completion must not start a second reviewer"
    );
    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("restart recovery should reconcile the current review handoff"),
        0,
        "an active Workspace Review is already durably owned and needs no replay"
    );
    let recovered_monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("read reviewer monitor after recovery")
        .expect("review monitor remains durable");
    assert_eq!(
        recovered_monitor.last_run_id.as_deref(),
        Some(reviewer_run_id.as_str())
    );
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read reviewer events after recovery")
            .into_iter()
            .filter(|event| event.step == "workspace_review" && event.status == "reviewing")
            .count(),
        reviewer_events_before,
        "restart recovery must not start a second reviewer"
    );

    let stale_run_id = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed stale repair run")
        .id;
    let stale = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, stale_run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Stale repair completion must not restart Workspace Review".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect_err("stale completion must fail closed");
    assert_eq!(stale.0, axum::http::StatusCode::CONFLICT);
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read reviewer events after stale completion")
            .into_iter()
            .filter(|event| event.step == "workspace_review" && event.status == "reviewing")
            .count(),
        reviewer_events_before,
        "a stale repair generation must not start a reviewer"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "a stale repair generation must not publish while review is pending"
    );

    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha,
        target.diff_fingerprint,
        Some(reviewer_run_id.clone()),
        ArtifactId::from_string("repair-review-artifact"),
        1,
        chrono::Utc::now(),
        None,
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("persist current Workspace Review monitor");

    let push_started = Arc::new(tokio::sync::Notify::new());
    *mock_github
        .push_branch_with_expected_remote_oid_lease_delay_ms
        .lock()
        .expect("exact push delay lock") = 50;
    *mock_github
        .push_branch_with_expected_remote_oid_lease_started
        .lock()
        .expect("exact push notification lock") = Some(Arc::clone(&push_started));
    let remote_path = fixture.remote_path.clone();
    let workspace_path = fixture.workspace_path.clone();
    let branch_name = fixture.branch_name.clone();
    let repaired_head = fixture.repaired_head.clone();
    let remote_update = tokio::spawn(async move {
        push_started.notified().await;
        let source_refspec =
            format!("refs/heads/{branch_name}:refs/ralphx-test/review-repair-source");
        git(
            &remote_path,
            &[
                "fetch",
                workspace_path.to_str().expect("workspace path"),
                &source_refspec,
            ],
        );
        git(
            &remote_path,
            &[
                "update-ref",
                &format!("refs/heads/{branch_name}"),
                &repaired_head,
            ],
        );
    });

    let Json(review) = Box::pin(complete_agent_workspace_review_run(
        axum::extract::State(state.clone()),
        Path(conversation_id.to_string()),
        Json(CompleteAgentWorkspaceReviewRunRequest {
            outcome: Some("passed".to_string()),
            summary: "Workspace Review passed".to_string(),
            blocker: None,
            created_by_run_id: Some(reviewer_run_id),
        }),
    ))
    .await
    .expect("passed Workspace Review should resume the repair continuation");
    remote_update.await.expect("remote update joins");

    assert_eq!(review.monitor.review_gate_status, "passed");
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0,
        "Workspace Review must not route a repair-owned branch through the normal publisher"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the passed review must continue the exact repair generation"
    );
    assert_eq!(mock_github.create_calls(), 1);
    assert!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("read settled repair attempt")
            .is_none(),
        "the passed review settles the repair only after its durable PR-monitor handoff"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read repair messages")
            .is_empty(),
        "review continuation must not emit duplicate completion messages"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "review continuation must not enqueue duplicate messages"
    );
}

#[tokio::test]
async fn failed_workspace_review_blocks_durable_repair_without_starting_or_publishing() {
    let conversation_id = ChatConversationId::from_string("67676767-6767-6767-6767-676767676767");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-review-repair-failed-gate",
    );
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let run_id = seed_checkpointed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: true,
            ..ReviewSettings::default()
        })
        .await
        .expect("require Workspace Review before the repair continuation");
    let workspace = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace for failed Workspace Review")
        .expect("workspace exists");
    let review_context = load_agent_workspace_review_context(&app_state, &workspace)
        .await
        .expect("load Workspace Review target");
    let target = review_context.target.expect("review target exists");
    let mut monitor = review_context.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha,
        target.diff_fingerprint,
        Some("failed-workspace-review-run".to_string()),
        ArtifactId::from_string("failed-workspace-review-artifact"),
        1,
        chrono::Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
    monitor.last_error = Some("Workspace reviewer runtime is unavailable".to_string());
    app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("persist failed Workspace Review monitor");
    let state = make_http_state(app_state);

    let completed = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Repair is complete but its required review already failed".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("failed Workspace Review gate should settle the durable repair truthfully")
    .0;

    assert_eq!(completed.status, "blocked");
    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load blocked durable repair")
        .expect("blocked repair remains current for an explicit retry");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        current
            .blocker
            .as_deref()
            .is_some_and(|blocker| blocker.contains("unavailable")),
        "the blocked durable attempt must retain the review failure detail"
    );
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read failed-review events")
            .into_iter()
            .all(|event| event.step != "workspace_review"),
        "a failed review gate must not start a replacement reviewer"
    );
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0,
        "a failed review gate must not enter the normal publisher"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "a failed review gate must not push the repair-owned branch"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "a failed review gate must not create or reconcile a pull request"
    );
}

#[tokio::test]
async fn unavailable_workspace_reviewer_blocks_durable_repair_without_publishing() {
    let conversation_id = ChatConversationId::from_string("68686868-6868-6868-6868-686868686868");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-review-repair-unavailable-reviewer",
    );
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let run_id = seed_checkpointed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: true,
            ..ReviewSettings::default()
        })
        .await
        .expect("require Workspace Review before the repair continuation");
    let state = make_http_state(app_state);

    let completed = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Repair requires an unavailable Workspace Reviewer".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("unavailable Workspace Reviewer should block the repair truthfully")
    .0;

    assert_eq!(completed.status, "blocked");
    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("load blocked durable repair")
        .expect("blocked repair remains current for an explicit retry");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        current
            .blocker
            .as_deref()
            .is_some_and(|blocker| blocker.contains("could not start")),
        "the durable blocker must retain the unavailable reviewer cause"
    );
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read unavailable-review events")
            .into_iter()
            .all(|event| !(event.step == "workspace_review" && event.status == "reviewing")),
        "an unavailable reviewer must not create a successful reviewer-start event"
    );
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0,
        "an unavailable reviewer must not enter the normal publisher"
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "an unavailable reviewer must not push the repair-owned branch"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "an unavailable reviewer must not create or reconcile a pull request"
    );
}

#[tokio::test]
async fn repaired_auto_publish_continuation_uses_one_exact_lease_effect_and_push() {
    let conversation_id = ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
    let fixture =
        setup_rewritten_repair_publish_fixture(conversation_id, "project-repaired-auto-publish");
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let workspace_repo = Arc::clone(&app_state.agent_conversation_workspace_repo);
    app_state =
        app_state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    let run_id = seed_checkpointed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    let state = make_http_state(app_state);
    let repair_attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read seeded repair attempt")
        .expect("repair attempt starts current");

    let push_started = Arc::new(tokio::sync::Notify::new());
    *mock_github
        .push_branch_with_expected_remote_oid_lease_delay_ms
        .lock()
        .expect("exact push delay lock") = 50;
    *mock_github
        .push_branch_with_expected_remote_oid_lease_started
        .lock()
        .expect("exact push notification lock") = Some(Arc::clone(&push_started));
    let remote_path = fixture.remote_path.clone();
    let workspace_path = fixture.workspace_path.clone();
    let branch_name = fixture.branch_name.clone();
    let repaired_head = fixture.repaired_head.clone();
    let remote_update = tokio::spawn(async move {
        push_started.notified().await;
        let source_refspec = format!("refs/heads/{branch_name}:refs/ralphx-test/repair-source");
        git(
            &remote_path,
            &[
                "fetch",
                workspace_path.to_str().expect("workspace path"),
                &source_refspec,
            ],
        );
        git(
            &remote_path,
            &[
                "update-ref",
                &format!("refs/heads/{branch_name}"),
                &repaired_head,
            ],
        );
    });

    let continuation_gate = Arc::new(tokio::sync::Barrier::new(2));
    set_agent_workspace_repair_completion_continuation_gate_for_test(Arc::clone(
        &continuation_gate,
    ));
    let completion = tokio::spawn(complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Resolved the rebased workspace repair".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    ));
    continuation_gate.wait().await;

    let duplicate = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Duplicate completion must not republish".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("duplicate completion should be idempotent")
    .0;
    assert_eq!(duplicate.status, "already_completed");
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        0,
        "the duplicate must not enter the continuation before the owner"
    );
    assert!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read publication events after duplicate")
            .is_empty(),
        "the duplicate must not append workflow events"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read messages after duplicate")
            .is_empty(),
        "the duplicate must not create a chat message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "the duplicate must not enqueue a message"
    );

    continuation_gate.wait().await;
    let response = completion
        .await
        .expect("repair completion task joins")
        .expect("repair completion should succeed")
        .0;
    clear_agent_workspace_repair_completion_continuation_gate_for_test();

    assert_eq!(response.status, "accepted");
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0
    );
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the repaired branch must issue exactly one bounded exact-lease push"
    );
    remote_update.await.expect("remote update joins");

    let terminal_replay = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Terminal duplicate completion must not re-enter publish".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("terminal duplicate completion is idempotent")
    .0;
    assert_eq!(terminal_replay.status, "already_completed");
    assert_eq!(
        mock_github.create_calls(),
        1,
        "a terminal replay must not create or update another pull request"
    );

    let current_attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read repair attempt after PR handoff");
    assert!(
        current_attempt.is_none(),
        "the repair attempt must settle only after the PR monitoring handoff; current={current_attempt:?}, create_calls={}",
        mock_github.create_calls()
    );
    assert_eq!(
        mock_github.create_calls(),
        1,
        "the observed repair-owned push must enter the normal PR creation pipeline exactly once"
    );
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read workspace after PR handoff")
        .expect("workspace remains available");
    assert_eq!(workspace.publication_pr_number, Some(1));
    let effect_key = format!(
        "agent_workspace_repair:{}:{}:push_branch",
        repair_attempt.id, repair_attempt.generation
    );
    let effect = state
        .app_state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect_key)
        .await
        .expect("read durable push effect")
        .expect("continuation must checkpoint one durable push effect");
    assert_eq!(effect.status.as_str(), "observed");
    assert_eq!(
        effect.intended_head_oid.as_deref(),
        Some(fixture.repaired_head.as_str())
    );
    let pr_effect_key = format!(
        "agent_workspace_repair:{}:{}:create_pr",
        repair_attempt.id, repair_attempt.generation
    );
    let pr_effect = state
        .app_state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&pr_effect_key)
        .await
        .expect("read durable PR handoff effect")
        .expect("continuation must checkpoint its PR monitoring handoff");
    assert_eq!(pr_effect.status.as_str(), "observed");
    assert_eq!(pr_effect.expected_pr_number, Some(1));
    assert!(
        pr_effect
            .receipt_json
            .as_deref()
            .is_some_and(|receipt| receipt.contains("\"monitoring_handoff\":true")),
        "the durable PR receipt must be written only after normal publish starts monitoring"
    );
    assert!(
        state
            .app_state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&repair_attempt.id)
            .await
            .expect("read open effect")
            .is_none(),
        "the observed effect must not leave a duplicate continuation open"
    );
    let events = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read events after continuation");
    let published_events: Vec<_> = events
        .iter()
        .filter(|event| event.step == "published")
        .collect();
    assert_eq!(
        published_events.len(),
        1,
        "the normal publish event is emitted exactly once"
    );
    assert_eq!(published_events[0].status, "succeeded");
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read messages after continuation")
            .is_empty(),
        "the continuation does not emit a duplicate chat message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "the continuation does not enqueue a duplicate message"
    );
}

#[tokio::test]
async fn repaired_auto_publish_blocks_when_base_advances_before_pr_handoff() {
    let conversation_id = ChatConversationId::from_string("45454545-4545-4545-4545-454545454545");
    let fixture = setup_rewritten_repair_publish_fixture(
        conversation_id,
        "project-repaired-auto-publish-base-advance",
    );
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    let workspace_repo = Arc::clone(&app_state.agent_conversation_workspace_repo);
    app_state =
        app_state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    let run_id = seed_checkpointed_rewritten_repair_publish_workspace(&app_state, &fixture).await;
    let state = make_http_state(app_state);

    let push_started = Arc::new(tokio::sync::Notify::new());
    *mock_github
        .push_branch_with_expected_remote_oid_lease_delay_ms
        .lock()
        .expect("exact push delay lock") = 50;
    *mock_github
        .push_branch_with_expected_remote_oid_lease_started
        .lock()
        .expect("exact push notification lock") = Some(Arc::clone(&push_started));
    let remote_path = fixture.remote_path.clone();
    let workspace_path = fixture.workspace_path.clone();
    let branch_name = fixture.branch_name.clone();
    let repaired_head = fixture.repaired_head.clone();
    let base_sha = fixture.base_sha.clone();
    let remote_update = tokio::spawn(async move {
        push_started.notified().await;
        let source_refspec = format!("refs/heads/{branch_name}:refs/ralphx-test/repair-source");
        git(
            &remote_path,
            &[
                "fetch",
                workspace_path.to_str().expect("workspace path"),
                &source_refspec,
            ],
        );
        git(
            &remote_path,
            &[
                "update-ref",
                &format!("refs/heads/{branch_name}"),
                &repaired_head,
            ],
        );
        advance_bare_remote_base(&remote_path, &base_sha);
    });

    let response = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Resolved the rebased workspace repair".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("repair completion should preserve durable recovery state")
    .0;
    remote_update.await.expect("remote update joins");

    assert_eq!(response.status, "accepted");
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter"),
        1,
        "the repair-owned exact lease push remains the only branch mutation"
    );
    assert_eq!(
        *mock_github
            .push_branch_calls
            .lock()
            .expect("normal push counter"),
        0,
        "the normal publisher must not push a locally mutated stale branch"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "base drift after the repair push must prevent stale PR creation"
    );
    assert_eq!(
        *mock_github
            .patch_pr_metadata_calls
            .lock()
            .expect("PR metadata counter"),
        0,
        "base drift must prevent stale existing-PR updates as well"
    );
    let current = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read blocked repair attempt")
        .expect("base drift must leave a durable repair recovery action");
    assert_eq!(current.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(
        current
            .blocker
            .as_deref()
            .is_some_and(|blocker| blocker.contains("base")),
        "the durable blocker must truthfully describe the changed base authority"
    );
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read workspace")
        .expect("workspace remains available");
    assert_eq!(workspace.publication_pr_number, None);
    let events = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read publication events");
    assert!(
        events.iter().all(|event| event.step != "published"),
        "a stale remote branch must not emit a successful publication event"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read messages")
            .is_empty(),
        "the blocked continuation must not emit a completion message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "the blocked continuation must not enqueue a completion message"
    );
    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("restart recovery should preserve the durable base-drift blocker"),
        0,
        "restart recovery must not revive the fenced repair generation"
    );
    assert_eq!(mock_github.create_calls(), 0);
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter after restart recovery"),
        1,
        "restart recovery must not retry the fenced generation's exact lease push"
    );

    let replay = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Replay must not republish the stale branch".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("blocked replay should be idempotent")
    .0;
    assert_eq!(replay.status, "blocked");
    assert_eq!(mock_github.create_calls(), 0);
    assert_eq!(
        *mock_github
            .push_branch_with_expected_remote_oid_lease_calls
            .lock()
            .expect("exact push counter after replay"),
        1,
        "replay must not retry the stale generation's exact lease push"
    );
}

#[tokio::test]
async fn complete_update_only_repair_auto_publishes_when_enabled() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");

    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let conversation_id = ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    let mut project = Project::new(
        "Agent Workspace Update Repair".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-update-repair".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());

    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    let branch_name = "ralphx/test/agent-update-repair";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(workspace_path.join("repair.txt"), "repair\n").expect("write repair file");
    git(&workspace_path, &["add", "repair.txt"]);
    git(&workspace_path, &["commit", "-m", "repair workspace"]);
    let _repair_sha = git(&workspace_path, &["rev-parse", "HEAD"]);

    let github_trait: Arc<dyn GithubServiceTrait> = Arc::new(MockGithubService::new());
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = project.id.as_str().to_string();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha.clone()),
        branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.publication_pr_number = Some(391);
    workspace.publication_pr_url = Some("https://github.com/example/ralphx/pull/391".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_summary = Some("Workspace repair is in progress.".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_requested",
            "started",
            "Workspace agent repair requested before the base update can complete",
            Some("agent_fixable:update_only".to_string()),
        ))
        .await
        .expect("seed update-only repair request");
    let run_id = seed_current_repair_attempt(&app_state, conversation_id).await;
    checkpoint_current_repair_target_lease(
        &app_state,
        &conversation_id,
        &workspace_path,
        branch_name,
        &base_sha,
    )
    .await;
    disable_workspace_review_gate(&app_state).await;

    let state = make_http_state(app_state);
    let response = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Resolved the stale base repair".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("update-only repair completion should succeed")
    .0;

    assert_eq!(response.status, "accepted");

    let refreshed = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("query workspace")
        .expect("workspace exists");
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("refreshed")
    );
    assert_eq!(
        refreshed.pr_supervision_status.as_deref(),
        Some("publishing")
    );
    assert_eq!(refreshed.pr_auto_merge_current, Some(true));
}

#[tokio::test]
async fn complete_repair_uses_linked_plan_branch_for_ideation_workspace() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    let remote_path = repo.path().join("origin.git");

    git(
        repo.path(),
        &["init", "--bare", remote_path.to_str().expect("remote path")],
    );
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    git(
        repo.path(),
        &[
            "remote",
            "add",
            "origin",
            remote_path.to_str().expect("remote path"),
        ],
    );
    git(repo.path(), &["push", "-u", "origin", "main"]);

    let plan_branch_name = "ralphx/test/plan-repair";
    git(repo.path(), &["checkout", "-b", plan_branch_name, "main"]);
    std::fs::write(repo.path().join("plan.txt"), "repair\n").expect("write plan repair");
    git(repo.path(), &["add", "plan.txt"]);
    git(repo.path(), &["commit", "-m", "repair linked plan"]);
    let repair_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["checkout", "main"]);

    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let mut project = Project::new(
        "Ideation Workspace Repair".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-ideation-repair".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());

    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = project.id.as_str().to_string();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let session_id = IdeationSessionId::from_string("session-ideation-repair");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-ideation-repair");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-ideation-repair"),
        session_id.clone(),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.id = plan_branch_id.clone();
    plan_branch.pr_number = Some(90);
    plan_branch.pr_url = Some("https://github.com/mock/project/pull/90".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Failed;
    let plan_worktree_path =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch).unwrap();
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            plan_worktree_path
                .to_str()
                .expect("linked plan worktree path"),
            plan_branch_name,
        ],
    );
    app_state
        .plan_branch_repo
        .create(plan_branch.clone())
        .await
        .expect("seed plan branch");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha.clone()),
        plan_branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch_id.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let run_id = seed_current_repair_attempt(&app_state, conversation_id).await;
    checkpoint_current_repair_target_lease(
        &app_state,
        &conversation_id,
        repo.path(),
        plan_branch_name,
        &base_sha,
    )
    .await;
    disable_workspace_review_gate(&app_state).await;

    let state = make_http_state(app_state);
    let repair_attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read linked-plan repair attempt")
        .expect("linked-plan repair attempt starts current");
    let push_started = Arc::new(tokio::sync::Notify::new());
    *mock_github
        .push_branch_started
        .lock()
        .expect("linked-plan push notification lock") = Some(Arc::clone(&push_started));
    *mock_github
        .push_branch_delay_ms
        .lock()
        .expect("linked-plan push delay lock") = 1_000;
    let mut completion = tokio::spawn(complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Resolved the linked plan branch repair".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    ));
    tokio::select! {
        _ = push_started.notified() => {}
        completion_outcome = &mut completion => {
            let diagnostics = auto_publish_push_boundary_diagnostics(
                &state,
                &conversation_id,
                mock_github.as_ref(),
                &plan_worktree_path,
                plan_branch_name,
            )
            .await;
            panic!(
                "linked-plan completion returned before the push boundary; http_outcome={completion_outcome:?}; {diagnostics}"
            );
        }
        _ = tokio::time::sleep(Duration::from_secs(5)) => {
            let diagnostics = auto_publish_push_boundary_diagnostics(
                &state,
                &conversation_id,
                mock_github.as_ref(),
                &plan_worktree_path,
                plan_branch_name,
            )
            .await;
            panic!(
                "linked-plan completion did not reach the push boundary within five seconds; {diagnostics}"
            );
        }
    }
    recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
        .await
        .expect("overlapping recovery must leave the live linked-plan push authoritative");
    let after_overlapping_recovery = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read linked-plan attempt after overlapping recovery")
        .expect("the live linked-plan continuation remains current");
    assert_eq!(
        after_overlapping_recovery.phase,
        AgentWorkspaceRepairPhase::Continuing,
        "recovery must not block the generation while its exact durable push is in flight"
    );
    assert!(
        after_overlapping_recovery.blocker.is_none(),
        "the in-flight mutation claim is not an actionable repair blocker"
    );
    let source_refspec =
        format!("refs/heads/{plan_branch_name}:refs/ralphx-test/linked-plan-repair-source");
    git(
        &remote_path,
        &[
            "fetch",
            plan_worktree_path.to_str().expect("plan worktree path"),
            &source_refspec,
        ],
    );
    git(
        &remote_path,
        &[
            "update-ref",
            &format!("refs/heads/{plan_branch_name}"),
            &repair_sha,
        ],
    );
    let response = completion
        .await
        .expect("linked-plan repair completion task joins")
        .expect("ideation repair completion should succeed")
        .0;

    assert_eq!(response.status, "accepted");
    assert_eq!(
        mock_github.push_calls(),
        1,
        "the durable linked-plan continuation owns exactly one target-aware branch push"
    );
    let refreshed_plan_branch = state
        .app_state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .expect("query plan branch")
        .expect("plan branch exists");
    let continuation_attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read linked-plan continuation state");
    assert_eq!(
        refreshed_plan_branch.pr_push_status,
        PrPushStatus::Pushed,
        "overlapping recovery must leave the live continuation intact; current attempt: {continuation_attempt:?}"
    );
    let current_attempt = state
        .app_state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read linked-plan attempt after publication");
    assert!(
        current_attempt.is_none(),
        "the linked-plan repair settles only after its existing publication pipeline hands off monitoring"
    );
    let push_effect_key = format!(
        "agent_workspace_repair:{}:{}:push_branch",
        repair_attempt.id, repair_attempt.generation
    );
    let push_effect = state
        .app_state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&push_effect_key)
        .await
        .expect("read linked-plan push effect")
        .expect("linked-plan continuation checkpoints a durable push effect");
    assert_eq!(push_effect.status.as_str(), "observed");
    let pr_effect_key = format!(
        "agent_workspace_repair:{}:{}:update_pr",
        repair_attempt.id, repair_attempt.generation
    );
    let pr_effect = state
        .app_state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&pr_effect_key)
        .await
        .expect("read linked-plan PR handoff effect")
        .expect("linked-plan continuation checkpoints its PR monitoring handoff");
    assert_eq!(pr_effect.status.as_str(), "observed");
    assert_eq!(pr_effect.expected_pr_number, Some(90));
    let events = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("read linked-plan publication events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "published")
            .count(),
        1,
        "the linked-plan publisher records one publication event"
    );
    assert!(
        state
            .app_state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("read linked-plan messages")
            .is_empty(),
        "the linked-plan continuation does not create a completion message"
    );
    assert!(
        state.app_state.message_queue.list_keys().is_empty(),
        "the linked-plan continuation does not enqueue a completion message"
    );
    let replay = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        completion_headers(conversation_id, run_id),
        Json(CompleteAgentWorkspaceRepairRequest {
            summary: "Replay must not publish the linked plan twice".to_string(),
            blocker: None,
            resolution: None,
            reported_fix_commit_sha: None,
        }),
    )
    .await
    .expect("settled linked-plan completion replay is idempotent")
    .0;
    assert_eq!(replay.status, "already_completed");
    assert_eq!(
        mock_github.push_calls(),
        1,
        "a completion replay must not push the linked plan again"
    );
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("read linked-plan events after replay")
            .iter()
            .filter(|event| event.step == "published")
            .count(),
        1,
        "a completion replay must not add a second linked-plan publication event"
    );
    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(state.app_state.as_ref())
            .await
            .expect("restart recovery should accept the settled linked-plan handoff"),
        0,
        "restart recovery must not revive a settled linked-plan repair"
    );
    assert_eq!(
        mock_github.push_calls(),
        1,
        "restart recovery must not issue another linked-plan push"
    );
}

/// Regression for the PR-as-base deadlock: a workspace based on another PR's head branch has a
/// blocked repair whose durable target still names the drifted base. An explicit user base
/// selection must persist the new base (ref + freshly resolved commit) BEFORE retrying, so the
/// superseding generation targets the user's base instead of recapturing the stale one.
#[tokio::test]
async fn explicit_base_update_supersedes_blocked_repair_with_the_new_base() {
    let temp = tempfile::tempdir().expect("fixture root");
    let repo_path = temp.path().join("repo");
    let remote_path = temp.path().join("remote.git");
    let worktree_parent = temp.path().join("worktrees");
    git(
        temp.path(),
        &["init", "--bare", remote_path.to_str().expect("remote path")],
    );
    git(
        temp.path(),
        &["init", "-b", "main", repo_path.to_str().expect("repo path")],
    );
    git(&repo_path, &["config", "user.email", "test@example.com"]);
    git(&repo_path, &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write base file");
    git(&repo_path, &["add", "."]);
    git(&repo_path, &["commit", "-m", "base"]);
    git(
        &repo_path,
        &[
            "remote",
            "add",
            "origin",
            remote_path.to_str().expect("remote path"),
        ],
    );
    git(&repo_path, &["push", "-u", "origin", "main"]);

    // The sibling PR head branch used as the workspace base, later merged into main.
    git(&repo_path, &["checkout", "-b", "ralphx/pr-base"]);
    std::fs::write(repo_path.join("pr.txt"), "pr\n").expect("write pr file");
    git(&repo_path, &["add", "."]);
    git(&repo_path, &["commit", "-m", "pr work"]);
    git(&repo_path, &["push", "-u", "origin", "ralphx/pr-base"]);
    let pr_base_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);
    git(
        &repo_path,
        &["merge", "--no-ff", "ralphx/pr-base", "-m", "merge pr"],
    );
    git(&repo_path, &["push", "origin", "main"]);
    let merged_main_sha = git(&repo_path, &["rev-parse", "main"]);

    let mut project = Project::new(
        "Explicit blocked-repair retarget".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project persists");
    let conversation_id = ChatConversationId::new();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persists");

    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path resolves");
    let branch = format!("ralphx/test/agent-{}", &conversation_id.as_str()[..8]);
    GitService::create_worktree(&repo_path, &worktree_path, &branch, "ralphx/pr-base")
        .await
        .expect("create workspace worktree from the PR base branch");

    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "ralphx/pr-base".to_string(),
        Some("PR #941: base".to_string()),
        Some(pr_base_sha.clone()),
        branch.clone(),
        worktree_path.to_string_lossy().to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persists");

    let attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "ralphx/pr-base",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    let started = match state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "merge in existing worktree failed".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start blocked repair attempt")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) => started,
        outcome => panic!("expected a new repair attempt, got {outcome:?}"),
    };
    let mut blocked = started.clone();
    blocked.phase = AgentWorkspaceRepairPhase::Blocked;
    blocked.blocker = Some(
        "workspace repair push handoff base ref changed from 'main' to 'ralphx/pr-base'"
            .to_string(),
    );
    blocked.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: blocked,
            expected_phase: started.phase,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("block the seeded repair attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("expected the seeded attempt to block, got {outcome:?}"),
    }

    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();

    let response = update_agent_conversation_workspace_from_base_for_app_state_with_caller(
        &state,
        &execution_state,
        conversation_id,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
        None,
    )
    .await
    .expect("explicit base update supersedes the blocked repair");

    assert!(
        response.repair_started,
        "the explicit selection must route into a repair retry, not a silent no-op"
    );
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read workspace")
        .expect("workspace exists");
    assert_eq!(
        workspace.base_ref, "main",
        "the explicit selection must persist before the blocked-repair retry"
    );
    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(merged_main_sha.as_str()),
        "the persisted base commit must be freshly resolved from origin, not the stale PR base"
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("read current repair attempt")
        .expect("a successor repair attempt exists");
    assert_ne!(
        current.id, started.id,
        "the blocked generation must be superseded, not replayed"
    );
    assert_eq!(
        current.target_base_ref, "main",
        "the successor must target the user's explicit base"
    );
    assert_eq!(
        current.target_base_commit.as_deref(),
        Some(merged_main_sha.as_str()),
        "the successor must capture the fresh base commit"
    );
    let predecessor = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&started.id)
        .await
        .expect("read predecessor")
        .expect("predecessor remains for audit");
    assert!(
        predecessor.settled_at.is_some(),
        "the drifted generation must be durably settled"
    );
}
