use super::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use super::agent_workspace_publish_recovery::recover_agent_workspace_repair_attempts_for_state;
use super::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION;
use super::git_mutation_recovery::{
    recover_in_flight_git_mutations_for_state, recover_repair_owned_in_flight_git_mutations,
    GitMutationRecoveryOutcome,
};
use super::publish_resilience::{
    push_agent_workspace_repair_branch, AgentWorkspaceRepairPushOutcome,
    AgentWorkspaceRepairPushRequest,
};
use super::{AppState, GitService};
use chrono::{Duration, Utc};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind,
    AgentWorkspaceRepairEffectStatus, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
    ChatConversationId, GitTargetLeaseOwner, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentConversationWorkspaceRepository,
    AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
    AgentWorkspaceRepairRepository, BeginGitMutation, BranchUpdateRepository,
    CreateAgentWorkspaceRepairEffect, CreateAgentWorkspaceRepairEffectOutcome,
    StartOrJoinAgentWorkspaceRepairAttempt, StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::domain::services::GithubServiceTrait;
use crate::error::AppError;
use crate::infrastructure::memory::memory_agent_conversation_workspace_repo::ForcedCreateAgentWorkspaceRepairEffectOutcome;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryBranchUpdateRepository,
};
use crate::infrastructure::GhCliGithubService;
use crate::tests::mock_github_service::MockGithubService;

struct RepairPushTestState {
    agent_workspace_repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
}

struct RepairPushFixture {
    _temp: tempfile::TempDir,
    state: RepairPushTestState,
    memory_repair_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    project: Project,
    workspace: AgentConversationWorkspace,
    attempt: AgentWorkspaceRepairAttempt,
    remote_path: PathBuf,
    branch: String,
    local_head: String,
}

#[derive(Clone, Copy)]
enum RepairPushRemoteHistory {
    Absent,
    FastForward,
    Rewritten,
}

fn git(repo: &Path, args: &[&str]) -> String {
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

fn commit_empty(repo: &Path, message: &str) {
    git(repo, &["commit", "--allow-empty", "-m", message]);
}

async fn setup_workspace_push(remote_history: RepairPushRemoteHistory) -> RepairPushFixture {
    let temp = tempfile::tempdir().expect("temporary fixture root");
    let repository = temp.path().join("repository");
    let remote_path = temp.path().join("remote.git");
    let worktree_parent = temp.path().join("worktrees");
    git(
        temp.path(),
        &["init", "--bare", remote_path.to_str().expect("remote path")],
    );
    git(
        temp.path(),
        &[
            "init",
            "-b",
            "main",
            repository.to_str().expect("repo path"),
        ],
    );
    git(&repository, &["config", "user.email", "test@example.com"]);
    git(&repository, &["config", "user.name", "RalphX Test"]);
    commit_empty(&repository, "base");
    git(
        &repository,
        &[
            "remote",
            "add",
            "origin",
            remote_path.to_str().expect("remote path"),
        ],
    );
    git(&repository, &["push", "-u", "origin", "main"]);

    let mut project = Project::new(
        "Repair publish safety".to_string(),
        repository.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("repair-push-conversation");
    let branch = "ralphx/repair/publish-safety".to_string();
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("canonical workspace path");
    GitService::create_worktree(&repository, &worktree_path, &branch, "main")
        .await
        .expect("create owned workspace worktree");

    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        branch.clone(),
        worktree_path.to_string_lossy().to_string(),
    );
    let memory_repair_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = memory_repair_repo.clone();
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = memory_repair_repo.clone();
    let memory_branch_update_repo = Arc::new(MemoryBranchUpdateRepository::new());
    let branch_update_repo: Arc<dyn BranchUpdateRepository> = memory_branch_update_repo.clone();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("persist workspace");

    git(&worktree_path, &["push", "-u", "origin", &branch]);
    commit_empty(&worktree_path, "local repaired head");
    let local_head = git(&worktree_path, &["rev-parse", "HEAD"]);

    match remote_history {
        RepairPushRemoteHistory::Absent => {
            git(
                &remote_path,
                &["update-ref", "-d", &format!("refs/heads/{branch}")],
            );
            git(
                &worktree_path,
                &["update-ref", "-d", &format!("refs/remotes/origin/{branch}")],
            );
        }
        RepairPushRemoteHistory::FastForward => {}
        RepairPushRemoteHistory::Rewritten => {
            // Make origin diverge from the repaired local branch. This forces the production
            // path to choose the exact force-with-lease method while still proving the remote
            // OID first.
            git(
                &repository,
                &["checkout", "-b", "remote-repair-head", "main"],
            );
            commit_empty(&repository, "remote concurrent head");
            git(
                &repository,
                &[
                    "push",
                    "origin",
                    &format!("remote-repair-head:refs/heads/{branch}"),
                ],
            );
            git(&repository, &["checkout", "main"]);
        }
    }

    let attempt = AgentWorkspaceRepairAttempt::new(
        workspace.conversation_id.clone(),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    let attempt = match repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "publish repaired branch".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a new repair attempt, got {outcome:?}"),
    };
    let identity = GitService::canonical_target_identity(&worktree_path, &branch)
        .await
        .expect("resolve canonical repair target");
    let common_dir = identity.git_common_dir().to_string_lossy().into_owned();
    let target_ref = identity.full_ref().to_string();
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease { identity, owner })
        .await
        .expect("acquire durable repair target lease")
    else {
        panic!("new repair fixture should acquire its target lease");
    };
    let mut pending = attempt.clone();
    pending.phase = AgentWorkspaceRepairPhase::ContinuationPending;
    pending.git_common_dir = Some(common_dir);
    pending.target_ref = Some(target_ref);
    pending.target_identity_version = Some(AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION);
    pending.target_lease_epoch = Some(fencing_epoch);
    pending.updated_at += Duration::microseconds(1);
    let pending = match repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: pending,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: attempt.updated_at,
            next_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("enter continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected continuation transition, got {outcome:?}"),
    };

    RepairPushFixture {
        _temp: temp,
        state: RepairPushTestState {
            agent_workspace_repair_repo: repair_repo,
            branch_update_repo,
        },
        memory_repair_repo,
        project,
        workspace,
        attempt: pending,
        remote_path,
        branch,
        local_head,
    }
}

async fn setup_rewritten_workspace_push() -> RepairPushFixture {
    setup_workspace_push(RepairPushRemoteHistory::Rewritten).await
}

async fn state_with_in_flight_repair_push(
    fixture: &RepairPushFixture,
) -> (
    AppState,
    AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairEffect,
) {
    let mut state = AppState::new_test();
    state
        .project_repo
        .create(fixture.project.clone())
        .await
        .expect("persist repair project");
    state.agent_conversation_workspace_repo = fixture.memory_repair_repo.clone();
    state.agent_workspace_repair_repo = fixture.memory_repair_repo.clone();
    state.branch_update_repo = fixture.state.branch_update_repo.clone();

    let identity = GitService::canonical_target_identity(
        Path::new(&fixture.workspace.worktree_path),
        &fixture.branch,
    )
    .await
    .expect("resolve canonical repair target");
    let owner = GitTargetLeaseOwner::agent_workspace_repair(fixture.attempt.id.as_str());
    let fencing_epoch = match state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: owner.clone(),
        })
        .await
        .expect("acquire repair target lease")
    {
        AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch }
        | AcquireGitTargetLeaseOutcome::AlreadyOwned { fencing_epoch } => fencing_epoch,
        outcome => panic!("repair target lease should remain repair-owned, got {outcome:?}"),
    };

    let mut continuing = fixture.attempt.clone();
    continuing.phase = AgentWorkspaceRepairPhase::Continuing;
    continuing.git_common_dir = Some(identity.git_common_dir().to_string_lossy().into_owned());
    continuing.target_ref = Some(identity.full_ref().to_string());
    continuing.target_identity_version = Some(AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION);
    continuing.target_lease_epoch = Some(fencing_epoch);
    continuing.updated_at += Duration::microseconds(1);
    let continuing = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: continuing,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at: fixture.attempt.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("enter repair continuation")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected continuing repair attempt, got {outcome:?}"),
    };

    let remote_oid = git(
        &fixture.remote_path,
        &["rev-parse", &format!("refs/heads/{}", fixture.branch)],
    );
    let mut effect = AgentWorkspaceRepairEffect::new(
        continuing.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        repair_push_effect_idempotency_key(fixture),
        Utc::now(),
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = Some(fixture.local_head.clone());
    effect.expected_remote_oid = Some(remote_oid);
    let effect = match state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: continuing.id.clone(),
            generation: continuing.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: continuing.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist repair push intent")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected repair push intent, got {outcome:?}"),
    };
    let claim_id = format!("{}:push", effect.id);
    state
        .branch_update_repo
        .begin_git_mutation(BeginGitMutation {
            identity,
            owner,
            fencing_epoch,
            claim_id,
            kind: crate::domain::entities::GitMutationKind::Push,
        })
        .await
        .expect("persist in-flight repair mutation claim");

    (state, continuing, effect)
}

#[tokio::test]
async fn busy_repair_push_returns_before_touching_the_workspace_git_path() {
    let fixture = setup_rewritten_workspace_push().await;
    let (state, continuing, effect) = state_with_in_flight_repair_push(&fixture).await;
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    let outcome = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        AgentWorkspaceRepairPushRequest {
            target_worktree_path: Path::new("/definitely-missing-ralphx-repair-worktree"),
            target_branch_name: &fixture.branch,
            attempt: continuing,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
        },
    )
    .await
    .expect("the existing durable mutation claim should classify the re-entry as Busy");

    assert_eq!(outcome, AgentWorkspaceRepairPushOutcome::Busy);
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert!(state
        .branch_update_repo
        .get_target_lease(
            &GitService::canonical_target_identity(
                Path::new(&fixture.workspace.worktree_path),
                &fixture.branch,
            )
            .await
            .expect("resolve fixture target identity")
        )
        .await
        .expect("read repair lease")
        .expect("repair lease should remain present")
        .active_mutation()
        .is_some());
    assert_eq!(
        state
            .agent_workspace_repair_repo
            .get_repair_effect_by_idempotency_key(&repair_push_effect_idempotency_key(&fixture))
            .await
            .expect("read repair push effect")
            .expect("durable repair effect should remain present")
            .id,
        effect.id,
        "a Busy return must preserve the existing owner receipt"
    );
}

#[tokio::test]
async fn simultaneous_first_repair_pushes_create_one_preflight_owner_before_git_observation() {
    let fixture = setup_rewritten_workspace_push().await;
    assert!(fixture
        .state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&repair_push_effect_idempotency_key(&fixture))
        .await
        .expect("initial repair effect lookup")
        .is_none());

    let github = Arc::new(MockGithubService::new());
    let push_started = Arc::new(tokio::sync::Notify::new());
    {
        let mut github_state = github.state();
        github_state.push_branch_with_expected_remote_oid_lease_delay_ms = 50;
        github_state.push_branch_with_expected_remote_oid_lease_started =
            Some(Arc::clone(&push_started));
    }
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let first_github = Arc::clone(&github_trait);
    let first_repair_repo = Arc::clone(&fixture.state.agent_workspace_repair_repo);
    let first_branch_update_repo = Arc::clone(&fixture.state.branch_update_repo);
    let first_worktree = PathBuf::from(&fixture.workspace.worktree_path);
    let first_branch = fixture.branch.clone();
    let first_attempt = fixture.attempt.clone();
    let first = tokio::spawn(async move {
        push_agent_workspace_repair_branch(
            &first_github,
            first_repair_repo,
            first_branch_update_repo,
            AgentWorkspaceRepairPushRequest {
                target_worktree_path: &first_worktree,
                target_branch_name: &first_branch,
                attempt: first_attempt,
                expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            },
        )
        .await
    });

    let remote_update = tokio::spawn(update_remote_after_push_started(
        Arc::clone(&push_started),
        fixture.remote_path.clone(),
        PathBuf::from(&fixture.workspace.worktree_path),
        fixture.branch.clone(),
        fixture.local_head.clone(),
    ));
    push_started.notified().await;

    let continuing = fixture
        .state
        .agent_workspace_repair_repo
        .get_repair_attempt(&fixture.attempt.id)
        .await
        .expect("load first-time continuation owner")
        .expect("first continuation should remain durable");
    let loser = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        AgentWorkspaceRepairPushRequest {
            target_worktree_path: Path::new("/definitely-missing-ralphx-first-push-loser"),
            target_branch_name: &fixture.branch,
            attempt: continuing,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
        },
    )
    .await
    .expect("the first-time losing continuation must return before Git observation");
    assert_eq!(loser, AgentWorkspaceRepairPushOutcome::Busy);

    remote_update.await.expect("remote update joins");
    let owner = first
        .await
        .expect("first continuation task joins")
        .expect("first continuation succeeds");
    assert!(matches!(
        owner,
        AgentWorkspaceRepairPushOutcome::Observed { .. }
    ));
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        1,
        "only the first preflight owner may reach the GitHub push"
    );
}

#[tokio::test]
async fn startup_recovery_leaves_a_busy_repair_continuation_untouched() {
    let fixture = setup_rewritten_workspace_push().await;
    let (mut state, continuing, _effect) = state_with_in_flight_repair_push(&fixture).await;
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let events_before = state
        .agent_conversation_workspace_repo
        .list_publication_events(&fixture.workspace.conversation_id)
        .await
        .expect("read workspace events before recovery");

    assert_eq!(
        recover_agent_workspace_repair_attempts_for_state(&state)
            .await
            .expect("recover durable repair attempts"),
        0,
        "a Busy continuation is pending reconciliation, not a completed recovery"
    );
    assert_eq!(
        state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&fixture.workspace.conversation_id)
            .await
            .expect("read current repair attempt"),
        Some(continuing),
        "a Busy recovery must not block, transition, or otherwise replace the owning attempt"
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.workspace.conversation_id)
            .await
            .expect("read workspace events after recovery"),
        events_before,
        "a Busy recovery must not append a compatibility event"
    );
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
}

fn request<'a>(
    fixture: &'a RepairPushFixture,
    attempt: AgentWorkspaceRepairAttempt,
) -> AgentWorkspaceRepairPushRequest<'a> {
    AgentWorkspaceRepairPushRequest {
        target_worktree_path: Path::new(&fixture.workspace.worktree_path),
        target_branch_name: &fixture.workspace.branch_name,
        attempt,
        expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
    }
}

async fn update_remote_after_push_started(
    started: Arc<tokio::sync::Notify>,
    remote_path: PathBuf,
    workspace_path: PathBuf,
    branch: String,
    local_head: String,
) {
    started.notified().await;
    let source_refspec = format!("refs/heads/{branch}:refs/ralphx-test/repair-source");
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
            &format!("refs/heads/{branch}"),
            local_head.as_str(),
        ],
    );
}

async fn workspace_target_identity(
    fixture: &RepairPushFixture,
) -> crate::domain::entities::GitTargetIdentity {
    let workspace_path = resolve_agent_conversation_workspace_path(
        &fixture.project,
        &fixture.workspace.conversation_id,
    )
    .expect("canonical workspace path");
    GitService::canonical_target_identity(&workspace_path, &fixture.branch)
        .await
        .expect("canonical workspace target identity")
}

fn repair_push_effect_idempotency_key(fixture: &RepairPushFixture) -> String {
    format!(
        "agent_workspace_repair:{}:{}:push_branch",
        fixture.attempt.id, fixture.attempt.generation
    )
}

#[tokio::test]
async fn stale_dispatch_lease_epoch_rejects_repair_push_before_any_github_or_git_mutation() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let target_identity = workspace_target_identity(&fixture).await;
    let repair_owner = GitTargetLeaseOwner::agent_workspace_repair(fixture.attempt.id.as_str());
    let fencing_epoch = match fixture
        .state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner: repair_owner.clone(),
        })
        .await
        .expect("repair lease acquisition should succeed")
    {
        AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch }
        | AcquireGitTargetLeaseOutcome::AlreadyOwned { fencing_epoch } => fencing_epoch,
        outcome => panic!("repair fixture must own its canonical target lease, got {outcome:?}"),
    };
    let mut checkpointed = fixture.attempt.clone();
    checkpointed.git_common_dir = Some(
        target_identity
            .git_common_dir()
            .to_string_lossy()
            .to_string(),
    );
    checkpointed.target_ref = Some(target_identity.full_ref().to_string());
    checkpointed.target_identity_version = Some(
        crate::application::agent_workspace_publish_repair_state::AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION,
    );
    checkpointed.target_lease_epoch = Some(fencing_epoch);
    checkpointed.updated_at += Duration::microseconds(1);
    let checkpointed = match fixture
        .state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: checkpointed,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at: fixture.attempt.updated_at,
            next_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("checkpoint dispatch lease on durable repair attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected durable lease checkpoint, got {outcome:?}"),
    };
    fixture
        .state
        .branch_update_repo
        .release_target_lease(&target_identity, &repair_owner, fencing_epoch)
        .await
        .expect("release stale repair lease");
    let foreign_owner = GitTargetLeaseOwner::branch_update("newer-owner", "branch-update");
    assert!(matches!(
        fixture
            .state
            .branch_update_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity: target_identity.clone(),
                owner: foreign_owner.clone(),
            })
            .await
            .expect("newer owner should acquire target"),
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));
    let remote_before = remote_branch_oid(&fixture);

    let error = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, checkpointed),
    )
    .await
    .expect_err("a stale repair lease epoch must reject push authority");
    assert!(error.to_string().contains("stale") || error.to_string().contains("owned"));
    assert_eq!(remote_branch_oid(&fixture), remote_before);
    {
        let github_state = github.state();
        assert_eq!(github_state.push_branch_calls, 0);
        assert_eq!(
            github_state.push_branch_with_expected_remote_oid_lease_calls,
            0
        );
    }
    assert!(
        fixture
            .state
            .agent_workspace_repair_repo
            .get_repair_effect_by_idempotency_key(&repair_push_effect_idempotency_key(&fixture))
            .await
            .expect("repair effect lookup")
            .is_none(),
        "stale authority must prevent effect creation before any push or PR handoff"
    );
    let lease = fixture
        .state
        .branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("foreign lease should remain readable")
        .expect("foreign lease should remain");
    assert_eq!(lease.owner(), &foreign_owner);
    assert!(!lease.is_released());
}

fn remote_branch_oid(fixture: &RepairPushFixture) -> String {
    git(
        &fixture.remote_path,
        &["rev-parse", &format!("refs/heads/{}", fixture.branch)],
    )
}

async fn assert_normal_repair_push_uses_the_ordinary_route(
    remote_history: RepairPushRemoteHistory,
) {
    let fixture = setup_workspace_push(remote_history).await;
    let github = Arc::new(MockGithubService::new());
    let started = Arc::new(tokio::sync::Notify::new());
    {
        let mut state = github.state();
        state.push_branch_delay_ms = 50;
        state.push_branch_started = Some(Arc::clone(&started));
    }
    let remote_update = tokio::spawn(update_remote_after_push_started(
        started,
        fixture.remote_path.clone(),
        PathBuf::from(&fixture.workspace.worktree_path),
        fixture.branch.clone(),
        fixture.local_head.clone(),
    ));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    let outcome = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect("normal repaired push should reconcile from its remote postcondition");
    remote_update.await.expect("remote updater should complete");

    assert!(matches!(
        outcome,
        AgentWorkspaceRepairPushOutcome::Observed {
            reconciled_after_push_error: false,
            ..
        }
    ));
    let state = github.state();
    assert_eq!(state.push_branch_calls, 1);
    assert_eq!(
        state.last_push_branch_name.as_deref(),
        Some(fixture.branch.as_str())
    );
    assert_eq!(
        state.push_branch_with_expected_remote_oid_lease_calls, 0,
        "a non-rewritten repair must never choose the force-with-lease route"
    );
}

#[tokio::test]
async fn remote_absent_first_repair_push_uses_the_ordinary_github_route() {
    assert_normal_repair_push_uses_the_ordinary_route(RepairPushRemoteHistory::Absent).await;
}

#[tokio::test]
async fn fast_forward_repair_push_uses_the_ordinary_github_route() {
    assert_normal_repair_push_uses_the_ordinary_route(RepairPushRemoteHistory::FastForward).await;
}

async fn assert_late_effect_outcome_preserves_dispatch_lease(
    forced_outcome: ForcedCreateAgentWorkspaceRepairEffectOutcome,
) {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let target_identity = workspace_target_identity(&fixture).await;
    fixture
        .memory_repair_repo
        .force_next_create_repair_effect_outcome(forced_outcome);

    let outcome = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect("late effect outcome should settle as stale");

    assert_eq!(outcome, AgentWorkspaceRepairPushOutcome::Stale);
    let lease = fixture
        .state
        .branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("read target lease")
        .expect("dispatch target lease record");
    assert!(
        !lease.is_released(),
        "durable dispatch lease remains owned for recovery"
    );
    assert_eq!(
        lease.owner(),
        &GitTargetLeaseOwner::agent_workspace_repair(fixture.attempt.id.as_str())
    );
    assert!(lease.active_mutation().is_none());
    assert!(
        fixture
            .state
            .agent_workspace_repair_repo
            .get_repair_effect_by_idempotency_key(&repair_push_effect_idempotency_key(&fixture))
            .await
            .expect("repair effect lookup")
            .is_none(),
        "late stale outcomes must not create, observe, or complete a push effect"
    );
    assert!(
        fixture
            .memory_repair_repo
            .list_publication_events(&fixture.workspace.conversation_id)
            .await
            .expect("publication events")
            .is_empty(),
        "late stale outcomes must not emit publication events"
    );
    let github_state = github.state();
    assert_eq!(github_state.push_branch_calls, 0);
    assert_eq!(
        github_state.push_branch_with_expected_remote_oid_lease_calls,
        0
    );
}

#[tokio::test]
async fn late_stale_effect_creation_preserves_the_dispatch_target_lease() {
    assert_late_effect_outcome_preserves_dispatch_lease(
        ForcedCreateAgentWorkspaceRepairEffectOutcome::Stale,
    )
    .await;
}

#[tokio::test]
async fn late_missing_effect_creation_preserves_the_dispatch_target_lease() {
    assert_late_effect_outcome_preserves_dispatch_lease(
        ForcedCreateAgentWorkspaceRepairEffectOutcome::Missing,
    )
    .await;
}

#[tokio::test]
async fn late_stale_effect_creation_preserves_a_preexisting_same_attempt_lease() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let target_identity = workspace_target_identity(&fixture).await;
    let owner = GitTargetLeaseOwner::agent_workspace_repair(fixture.attempt.id.as_str());
    let acquired = fixture
        .state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner: owner.clone(),
        })
        .await
        .expect("pre-existing target lease acquisition");
    assert!(matches!(
        acquired,
        AcquireGitTargetLeaseOutcome::AlreadyOwned { .. }
    ));
    fixture
        .memory_repair_repo
        .force_next_create_repair_effect_outcome(
            ForcedCreateAgentWorkspaceRepairEffectOutcome::Stale,
        );

    let outcome = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect("late stale outcome should not release another invocation's lease");

    assert_eq!(outcome, AgentWorkspaceRepairPushOutcome::Stale);
    let lease = fixture
        .state
        .branch_update_repo
        .get_target_lease(&target_identity)
        .await
        .expect("read target lease")
        .expect("pre-existing target lease record");
    assert!(!lease.is_released());
    assert_eq!(lease.owner(), &owner);
    assert!(lease.active_mutation().is_none());
    assert!(fixture
        .state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&repair_push_effect_idempotency_key(&fixture))
        .await
        .expect("repair effect lookup")
        .is_none());
    let github_state = github.state();
    assert_eq!(github_state.push_branch_calls, 0);
    assert_eq!(
        github_state.push_branch_with_expected_remote_oid_lease_calls,
        0
    );
}

#[tokio::test]
async fn reconciles_a_successful_exact_lease_push_from_the_verified_remote_postcondition() {
    let fixture = setup_rewritten_workspace_push().await;
    let expected_remote_oid = remote_branch_oid(&fixture);
    let github = Arc::new(MockGithubService::new());
    let started = Arc::new(tokio::sync::Notify::new());
    {
        let mut state = github.state();
        state.push_branch_with_expected_remote_oid_lease_delay_ms = 50;
        state.push_branch_with_expected_remote_oid_lease_started = Some(Arc::clone(&started));
    }
    let remote_update = tokio::spawn(update_remote_after_push_started(
        started,
        fixture.remote_path.clone(),
        PathBuf::from(&fixture.workspace.worktree_path),
        fixture.branch.clone(),
        fixture.local_head.clone(),
    ));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    let outcome = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect("verified remote receipt should settle the effect");
    remote_update.await.expect("remote updater should complete");

    let AgentWorkspaceRepairPushOutcome::Observed {
        effect,
        remote_oid,
        reconciled_after_push_error,
    } = outcome
    else {
        panic!("expected observed push receipt");
    };
    assert_eq!(remote_oid, fixture.local_head);
    assert_eq!(effect.status, AgentWorkspaceRepairEffectStatus::Observed);
    assert!(effect.completed_at.is_some());
    assert!(!reconciled_after_push_error);
    let state = github.state();
    assert_eq!(state.push_branch_calls, 0);
    assert_eq!(state.push_branch_with_expected_remote_oid_lease_calls, 1);
    assert_eq!(
        state
            .last_push_branch_with_expected_remote_oid_lease_args
            .as_ref()
            .map(|(local_ref, expected_oid)| (local_ref.as_str(), expected_oid.as_str())),
        Some((
            "refs/heads/ralphx/repair/publish-safety",
            expected_remote_oid.as_str()
        )),
        "only the rewritten branch may use the exact expected-OID force-with-lease route"
    );
}

#[tokio::test]
async fn real_git_rejects_a_mismatched_expected_lease_without_rewriting_the_remote_ref() {
    let fixture = setup_rewritten_workspace_push().await;
    let remote_before = remote_branch_oid(&fixture);
    assert_ne!(remote_before, fixture.local_head);
    let mismatched_expected_oid = if remote_before == "f".repeat(40) {
        "e".repeat(40)
    } else {
        "f".repeat(40)
    };
    let service = GhCliGithubService::new();
    let error = service
        .push_branch_with_expected_remote_oid_lease(
            Path::new(&fixture.workspace.worktree_path),
            &format!("refs/heads/{}", fixture.branch),
            &mismatched_expected_oid,
        )
        .await
        .expect_err("a mismatched force-with-lease expectation must reject the remote update");

    assert!(
        error.to_string().contains("git exited with code"),
        "the production Git runner must surface the rejected force-with-lease mutation"
    );
    assert_eq!(
        remote_branch_oid(&fixture),
        remote_before,
        "a rejected expected-OID lease must leave the remote ref byte-for-byte unchanged"
    );
}

#[tokio::test]
async fn ambiguous_exact_lease_failure_reconciles_only_when_origin_reaches_the_intended_head() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let started = Arc::new(tokio::sync::Notify::new());
    {
        let mut state = github.state();
        state.push_branch_with_expected_remote_oid_lease_delay_ms = 50;
        state.push_branch_with_expected_remote_oid_lease_started = Some(Arc::clone(&started));
        state.push_branch_with_expected_remote_oid_lease_result = Some(Err(
            AppError::Infrastructure("connection dropped after push".to_string()),
        ));
    }
    let remote_update = tokio::spawn(update_remote_after_push_started(
        started,
        fixture.remote_path.clone(),
        PathBuf::from(&fixture.workspace.worktree_path),
        fixture.branch.clone(),
        fixture.local_head.clone(),
    ));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    let outcome = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect("ambiguous error must reconcile from a satisfied postcondition");
    remote_update.await.expect("remote updater should complete");

    assert!(matches!(
        outcome,
        AgentWorkspaceRepairPushOutcome::Observed {
            reconciled_after_push_error: true,
            ..
        }
    ));
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        1
    );
}

#[tokio::test]
async fn ambiguous_exact_lease_failure_without_the_remote_postcondition_stays_in_flight() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    github
        .state()
        .push_branch_with_expected_remote_oid_lease_result = Some(Err(AppError::Infrastructure(
        "connection dropped before push".to_string(),
    )));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    let error = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect_err("unsatisfied postcondition must not be mistaken for a completed push");
    assert!(error.to_string().contains("connection dropped"));
    let effect = fixture
        .state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&format!(
            "agent_workspace_repair:{}:{}:push_branch",
            fixture.attempt.id, fixture.attempt.generation
        ))
        .await
        .expect("effect lookup")
        .expect("intent checkpoint");
    assert_eq!(effect.status, AgentWorkspaceRepairEffectStatus::InFlight);
    assert!(effect.completed_at.is_none());
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        1
    );
}

#[tokio::test]
async fn observed_push_receipt_is_reused_on_restart_without_a_second_mutation() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let started = Arc::new(tokio::sync::Notify::new());
    {
        let mut state = github.state();
        state.push_branch_with_expected_remote_oid_lease_delay_ms = 50;
        state.push_branch_with_expected_remote_oid_lease_started = Some(Arc::clone(&started));
    }
    let remote_update = tokio::spawn(update_remote_after_push_started(
        started,
        fixture.remote_path.clone(),
        PathBuf::from(&fixture.workspace.worktree_path),
        fixture.branch.clone(),
        fixture.local_head.clone(),
    ));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let first = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect("first push reconciliation");
    remote_update.await.expect("remote updater should complete");
    assert!(matches!(
        first,
        AgentWorkspaceRepairPushOutcome::Observed { .. }
    ));

    let restarted_attempt = fixture
        .state
        .agent_workspace_repair_repo
        .get_repair_attempt(&fixture.attempt.id)
        .await
        .expect("attempt read")
        .expect("attempt remains current");
    let second = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        AgentWorkspaceRepairPushRequest {
            target_worktree_path: Path::new(&fixture.workspace.worktree_path),
            target_branch_name: &fixture.workspace.branch_name,
            attempt: restarted_attempt,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
        },
    )
    .await
    .expect("restart should reuse the verified receipt");
    assert!(matches!(
        second,
        AgentWorkspaceRepairPushOutcome::Observed { .. }
    ));
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        1,
        "an observed receipt must prevent a duplicate force-with-lease mutation"
    );
}

#[tokio::test]
async fn stale_authority_and_wrong_remote_expectation_are_side_effect_free() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    let mut newer = fixture.attempt.clone();
    newer.summary = Some("newer owner update".to_string());
    newer.updated_at += Duration::microseconds(1);
    let transition = fixture
        .state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: newer,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_updated_at: fixture.attempt.updated_at,
            next_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("advance same-phase owner record");
    let current_attempt = match transition {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected current attempt, got {outcome:?}"),
    };
    let stale = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect("stale authority classification");
    assert_eq!(stale, AgentWorkspaceRepairPushOutcome::Stale);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(github.state().push_branch_calls, 0);

    let mut foreign_workspace = fixture.workspace.clone();
    foreign_workspace.branch_name = "ralphx/foreign/branch".to_string();
    let foreign_error = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        AgentWorkspaceRepairPushRequest {
            target_worktree_path: Path::new(&foreign_workspace.worktree_path),
            target_branch_name: &foreign_workspace.branch_name,
            attempt: current_attempt,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
        },
    )
    .await
    .expect_err("foreign workspace ref must be rejected before any push");
    assert!(foreign_error.to_string().contains("differs"));
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(github.state().push_branch_calls, 0);
}

#[tokio::test]
async fn foreign_git_target_lease_owner_blocks_the_push_without_a_mutation() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let workspace_path = resolve_agent_conversation_workspace_path(
        &fixture.project,
        &fixture.workspace.conversation_id,
    )
    .expect("canonical workspace path");
    let identity = GitService::canonical_target_identity(&workspace_path, &fixture.branch)
        .await
        .expect("canonical branch identity");
    let repair_owner = GitTargetLeaseOwner::agent_workspace_repair(fixture.attempt.id.as_str());
    fixture
        .state
        .branch_update_repo
        .release_target_lease(
            &identity,
            &repair_owner,
            fixture
                .attempt
                .target_lease_epoch
                .expect("repair fixture lease epoch"),
        )
        .await
        .expect("release fixture repair lease before installing foreign owner");
    let acquired = fixture
        .state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: GitTargetLeaseOwner::agent_workspace_repair("foreign-attempt"),
        })
        .await
        .expect("foreign lease acquisition");
    assert!(matches!(
        acquired,
        AcquireGitTargetLeaseOutcome::Acquired { .. }
    ));

    let error = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect_err("foreign target authority must block a repair push");
    assert!(error.to_string().contains("owned"));
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(github.state().push_branch_calls, 0);
    let lease = fixture
        .state
        .branch_update_repo
        .get_target_lease(&identity)
        .await
        .expect("read foreign target lease")
        .expect("foreign target lease record");
    assert!(!lease.is_released());
    assert_eq!(
        lease.owner(),
        &GitTargetLeaseOwner::agent_workspace_repair("foreign-attempt")
    );
    assert!(lease.active_mutation().is_none());
}

#[tokio::test]
async fn wrong_expected_remote_oid_fails_closed_before_the_exact_lease_mutation() {
    let fixture = setup_rewritten_workspace_push().await;
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let idempotency_key = format!(
        "agent_workspace_repair:{}:{}:push_branch",
        fixture.attempt.id, fixture.attempt.generation
    );
    let mut effect = AgentWorkspaceRepairEffect::new(
        fixture.attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        idempotency_key,
        Utc::now(),
    );
    effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    effect.intended_head_oid = Some(fixture.local_head.clone());
    effect.expected_remote_oid = Some("f".repeat(40));
    let created = fixture
        .state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: fixture.attempt.id.clone(),
            generation: fixture.attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_attempt_updated_at: fixture.attempt.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist wrong expectation checkpoint");
    assert!(matches!(
        created,
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    let error = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&fixture.state.agent_workspace_repair_repo),
        Arc::clone(&fixture.state.branch_update_repo),
        request(&fixture, fixture.attempt.clone()),
    )
    .await
    .expect_err("remote OID drift must block rather than force");
    assert!(error.to_string().contains("drifted"));
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(github.state().push_branch_calls, 0);
}

#[tokio::test]
async fn missing_remote_or_oid_expectations_fail_closed_before_any_push() {
    let absent_fixture = setup_workspace_push(RepairPushRemoteHistory::Absent).await;
    let absent_github = Arc::new(MockGithubService::new());
    let absent_github_trait: Arc<dyn GithubServiceTrait> = absent_github.clone();
    let mut absent_effect = AgentWorkspaceRepairEffect::new(
        absent_fixture.attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        repair_push_effect_idempotency_key(&absent_fixture),
        Utc::now(),
    );
    absent_effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    absent_effect.intended_head_oid = Some(absent_fixture.local_head.clone());
    absent_effect.expected_remote_oid = Some("a".repeat(40));
    let absent_created = absent_fixture
        .state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: absent_fixture.attempt.id.clone(),
            generation: absent_fixture.attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_attempt_updated_at: absent_fixture.attempt.updated_at,
            effect: absent_effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist an effect requiring a remote OID");
    assert!(matches!(
        absent_created,
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    let absent_error = push_agent_workspace_repair_branch(
        &absent_github_trait,
        Arc::clone(&absent_fixture.state.agent_workspace_repair_repo),
        Arc::clone(&absent_fixture.state.branch_update_repo),
        request(&absent_fixture, absent_fixture.attempt.clone()),
    )
    .await
    .expect_err("a missing remote ref must not satisfy a present-OID receipt");
    assert!(absent_error.to_string().contains("drifted"));
    assert_eq!(absent_github.state().push_branch_calls, 0);
    assert_eq!(
        absent_github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );

    let oid_fixture = setup_rewritten_workspace_push().await;
    let oid_github = Arc::new(MockGithubService::new());
    let oid_github_trait: Arc<dyn GithubServiceTrait> = oid_github.clone();
    let mut oid_effect = AgentWorkspaceRepairEffect::new(
        oid_fixture.attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        repair_push_effect_idempotency_key(&oid_fixture),
        Utc::now(),
    );
    oid_effect.status = AgentWorkspaceRepairEffectStatus::InFlight;
    oid_effect.intended_head_oid = Some(oid_fixture.local_head.clone());
    oid_effect.expected_remote_absent = false;
    let oid_created = oid_fixture
        .state
        .agent_workspace_repair_repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: oid_fixture.attempt.id.clone(),
            generation: oid_fixture.attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::ContinuationPending,
            expected_attempt_updated_at: oid_fixture.attempt.updated_at,
            effect: oid_effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist a malformed missing-OID effect");
    assert!(matches!(
        oid_created,
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    let oid_error = push_agent_workspace_repair_branch(
        &oid_github_trait,
        Arc::clone(&oid_fixture.state.agent_workspace_repair_repo),
        Arc::clone(&oid_fixture.state.branch_update_repo),
        request(&oid_fixture, oid_fixture.attempt.clone()),
    )
    .await
    .expect_err("a missing expected remote OID must fail closed when origin has the branch");
    assert!(oid_error.to_string().contains("partially initialized"));
    assert_eq!(oid_github.state().push_branch_calls, 0);
    assert_eq!(
        oid_github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
}

#[tokio::test]
async fn repair_claim_recovery_clears_an_unobserved_push_once_then_reuses_the_receipt_on_restart() {
    let fixture = setup_rewritten_workspace_push().await;
    let (mut state, continuing, effect) = state_with_in_flight_repair_push(&fixture).await;
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());

    let recovered = recover_repair_owned_in_flight_git_mutations(&state)
        .await
        .expect("recover the crash-before-push claim");
    assert_eq!(
        recovered,
        vec![GitMutationRecoveryOutcome::Cleared {
            claim_id: format!("{}:push", effect.id),
        }]
    );
    assert!(state
        .branch_update_repo
        .get_target_lease(
            &GitService::canonical_target_identity(
                Path::new(&fixture.workspace.worktree_path),
                &fixture.branch,
            )
            .await
            .expect("resolve repair target"),
        )
        .await
        .expect("load target lease")
        .expect("target lease remains owned")
        .active_mutation()
        .is_none());
    assert_eq!(
        state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&continuing.id)
            .await
            .expect("load push intent")
            .expect("unobserved intent remains retryable")
            .status,
        AgentWorkspaceRepairEffectStatus::InFlight
    );

    let started = Arc::new(tokio::sync::Notify::new());
    github
        .state()
        .push_branch_with_expected_remote_oid_lease_started = Some(Arc::clone(&started));
    let remote_update = tokio::spawn(update_remote_after_push_started(
        started,
        fixture.remote_path.clone(),
        PathBuf::from(&fixture.workspace.worktree_path),
        fixture.branch.clone(),
        fixture.local_head.clone(),
    ));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let first = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        AgentWorkspaceRepairPushRequest {
            target_worktree_path: Path::new(&fixture.workspace.worktree_path),
            target_branch_name: &fixture.branch,
            attempt: continuing,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
        },
    )
    .await
    .expect("resume the one unobserved push");
    remote_update.await.expect("remote push should complete");
    assert!(matches!(
        first,
        AgentWorkspaceRepairPushOutcome::Observed { .. }
    ));

    let restarted = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&fixture.attempt.id)
        .await
        .expect("load durable attempt")
        .expect("attempt remains current");
    let replay = push_agent_workspace_repair_branch(
        &github_trait,
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        AgentWorkspaceRepairPushRequest {
            target_worktree_path: Path::new(&fixture.workspace.worktree_path),
            target_branch_name: &fixture.branch,
            attempt: restarted,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
        },
    )
    .await
    .expect("replay must reuse the observed receipt");
    assert!(matches!(
        replay,
        AgentWorkspaceRepairPushOutcome::Observed { .. }
    ));
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        1,
        "crash recovery must not duplicate the resumed push"
    );
    assert_eq!(github.state().create_draft_pr_calls, 0);
    assert!(recover_repair_owned_in_flight_git_mutations(&state)
        .await
        .expect("repeated recovery")
        .is_empty());
}

#[tokio::test]
async fn repair_claim_recovery_observes_an_exact_push_without_a_second_git_or_github_effect() {
    let fixture = setup_rewritten_workspace_push().await;
    let (mut state, continuing, effect) = state_with_in_flight_repair_push(&fixture).await;
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    git(
        Path::new(&fixture.workspace.worktree_path),
        &["push", "--force", "origin", &fixture.branch],
    );

    let recovered = recover_repair_owned_in_flight_git_mutations(&state)
        .await
        .expect("recover the observed push claim");
    assert_eq!(
        recovered,
        vec![GitMutationRecoveryOutcome::Cleared {
            claim_id: format!("{}:push", effect.id),
        }]
    );
    let observed = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&repair_push_effect_idempotency_key(&fixture))
        .await
        .expect("read observed receipt")
        .expect("receipt remains durable");
    assert_eq!(observed.status, AgentWorkspaceRepairEffectStatus::Observed);
    assert!(observed.completed_at.is_some());
    assert_eq!(
        state
            .agent_workspace_repair_repo
            .get_repair_attempt(&continuing.id)
            .await
            .expect("read continuing attempt")
            .expect("attempt remains current")
            .phase,
        AgentWorkspaceRepairPhase::Continuing
    );
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(github.state().create_draft_pr_calls, 0);
    assert!(recover_repair_owned_in_flight_git_mutations(&state)
        .await
        .expect("replay the observed recovery")
        .is_empty());
}

#[tokio::test]
async fn repair_claim_recovery_blocks_an_ambiguous_remote_oid_without_side_effects() {
    let fixture = setup_rewritten_workspace_push().await;
    let (mut state, continuing, effect) = state_with_in_flight_repair_push(&fixture).await;
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let unrelated_oid = git(
        Path::new(&fixture.project.working_directory),
        &["rev-parse", "main"],
    );
    git(
        &fixture.remote_path,
        &[
            "update-ref",
            &format!("refs/heads/{}", fixture.branch),
            &unrelated_oid,
        ],
    );

    let recovered = recover_in_flight_git_mutations_for_state(&state)
        .await
        .expect("startup recovery should block an ambiguous OID safely");
    assert!(matches!(
        recovered.as_slice(),
        [GitMutationRecoveryOutcome::NeedsRepair { claim_id, reason }]
            if *claim_id == format!("{}:push", effect.id)
                && reason.contains("does not match")
    ));
    let blocked = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&continuing.id)
        .await
        .expect("read blocked repair")
        .expect("durable repair remains visible");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(blocked
        .blocker
        .as_deref()
        .unwrap_or_default()
        .contains("does not match"));
    assert_eq!(
        state
            .agent_workspace_repair_repo
            .get_repair_effect_by_idempotency_key(&repair_push_effect_idempotency_key(&fixture))
            .await
            .expect("read failed intent")
            .expect("failed intent remains auditable")
            .status,
        AgentWorkspaceRepairEffectStatus::Failed
    );
    let lease = state
        .branch_update_repo
        .get_target_lease(
            &GitService::canonical_target_identity(
                Path::new(&fixture.workspace.worktree_path),
                &fixture.branch,
            )
            .await
            .expect("resolve ambiguous repair target"),
        )
        .await
        .expect("load settled ambiguous repair lease")
        .expect("repair lease remains auditable");
    assert!(lease.is_released());
    assert!(lease.active_mutation().is_none());
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(github.state().create_draft_pr_calls, 0);
    assert!(recover_in_flight_git_mutations_for_state(&state)
        .await
        .expect("blocked startup replay remains idempotent")
        .is_empty());
}

#[tokio::test]
async fn repair_claim_recovery_blocks_a_stale_fencing_epoch_without_git_or_github_effects() {
    let fixture = setup_rewritten_workspace_push().await;
    let (mut state, continuing, effect) = state_with_in_flight_repair_push(&fixture).await;
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let mut stale = continuing.clone();
    stale.target_lease_epoch = stale.target_lease_epoch.map(|epoch| epoch + 1);
    stale.updated_at += Duration::microseconds(1);
    let stale = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: stale,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_updated_at: continuing.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Continuing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist stale epoch fixture")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected stale epoch attempt, got {outcome:?}"),
    };

    let recovered = recover_in_flight_git_mutations_for_state(&state)
        .await
        .expect("startup recovery should fail closed for a stale epoch");
    assert!(matches!(
        recovered.as_slice(),
        [GitMutationRecoveryOutcome::NeedsRepair { claim_id, reason }]
            if *claim_id == format!("{}:push", effect.id)
                && reason.contains("lease proof failed")
    ));
    let blocked = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&stale.id)
        .await
        .expect("read blocked stale attempt")
        .expect("attempt remains durable");
    assert_eq!(blocked.phase, AgentWorkspaceRepairPhase::Blocked);
    let lease = state
        .branch_update_repo
        .get_target_lease(
            &GitService::canonical_target_identity(
                Path::new(&fixture.workspace.worktree_path),
                &fixture.branch,
            )
            .await
            .expect("resolve stale repair target"),
        )
        .await
        .expect("load stale repair lease")
        .expect("stale claim must retain its exact lease");
    assert!(!lease.is_released());
    assert!(lease.active_mutation().is_some());
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(
        github
            .state()
            .push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    assert_eq!(github.state().create_draft_pr_calls, 0);
    let replay = recover_in_flight_git_mutations_for_state(&state)
        .await
        .expect("stale startup replay must remain side-effect free");
    assert!(matches!(
        replay.as_slice(),
        [GitMutationRecoveryOutcome::NeedsRepair { .. }]
    ));
    assert_eq!(github.state().push_branch_calls, 0);
}
