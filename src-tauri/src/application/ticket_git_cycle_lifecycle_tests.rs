use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;

use super::agent_conversation_workspace::{
    prepare_agent_conversation_workspace_with_setup_mode_defaults_branch_name_hint_and_linked_target,
    AgentConversationWorkspaceBaseSelection, AgentConversationWorkspaceBranchNameHint,
    AgentConversationWorkspacePrAutomationDefaults, AgentConversationWorkspaceSetupMode,
};
use super::ticket_git_cycle_lifecycle::{
    mark_strict_ticket_cycle_terminal, prepare_merged_strict_ticket_cycle_for_start,
    rollover_strict_ticket_workspace,
};
use super::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
    TicketCanonicalBranch, TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState,
    TicketGitConventionSnapshot,
};

const BRANCH: &str = "eng-42_ticket_ada";

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output should be UTF-8")
        .trim()
        .to_string()
}

fn init_repo() -> (tempfile::TempDir, PathBuf, Project, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let origin = temp.path().join("origin.git");
    let worktrees = temp.path().join("worktrees");
    git(temp.path(), &["init", "--bare", origin.to_str().unwrap()]);
    std::fs::create_dir_all(&repo).expect("repo directory");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-u", "origin", "main"]);
    git(&repo, &["branch", BRANCH, "main"]);
    git(&repo, &["push", "-u", "origin", BRANCH]);

    let mut project = Project::new(
        "Strict lifecycle".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktrees.to_string_lossy().to_string());
    (temp, repo, project, base)
}

fn strict_binding(project: &Project, base: &str) -> TicketCanonicalBranch {
    let mut binding = TicketCanonicalBranch::new_strict(
        project.id.clone(),
        "clickup",
        "ENG-42",
        BRANCH,
        "main",
        Some(base.to_string()),
        TicketGitConventionSnapshot {
            policy_version: 1,
            task_title: "Ticket".to_string(),
            username: Some("Ada".to_string()),
            commit_subject_rule: "ENG-42 - :summary:".to_string(),
            pr_title: "ENG-42 - Ticket".to_string(),
        },
        Utc::now(),
    );
    binding.origin_pushed = true;
    binding.cycle = TicketCanonicalBranchCycle {
        generation: 1,
        state: TicketCanonicalBranchCycleState::Active,
        base_commit: Some(base.to_string()),
        effective_merge_base: Some(base.to_string()),
        started_at: Some(Utc::now()),
        terminal_at: None,
    };
    binding
}

async fn strict_workspace(project: &Project, base: &str) -> AgentConversationWorkspace {
    let conversation_id = ChatConversationId::from_string("strict-cycle-conversation".to_string());
    let mut workspace =
        prepare_agent_conversation_workspace_with_setup_mode_defaults_branch_name_hint_and_linked_target(
            project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
                base_ref: Some(BRANCH.to_string()),
                display_name: Some("ClickUp ENG-42".to_string()),
                source_pull_request: None,
            },
            AgentConversationWorkspaceSetupMode::Blocking,
            AgentConversationWorkspacePrAutomationDefaults::default(),
            false,
            Some(AgentConversationWorkspaceBranchNameHint {
                provider: "clickup".to_string(),
                ticket_token: "ENG-42".to_string(),
            }),
            Some("main".to_string()),
        )
        .await
        .expect("strict linked workspace");
    workspace.base_commit = Some(base.to_string());
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_url = Some("https://example.test/pr/42".to_string());
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace
}

#[tokio::test]
async fn terminal_outcome_advances_strict_cycle_once_and_closed_unmerged_stays_blocked() {
    let (_temp, _repo, project, base) = init_repo();
    let state = AppState::new_test();
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let mut workspace = strict_workspace(&project, &base).await;

    let merged = mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .expect("merged cycle should persist")
    .expect("strict binding");
    assert_eq!(merged.cycle.state, TicketCanonicalBranchCycleState::Merged);
    assert!(merged.cycle.terminal_at.is_some());

    let repeated = mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .expect("repeated terminalization should be idempotent")
    .expect("strict binding");
    assert_eq!(repeated.cycle.generation, merged.cycle.generation);

    workspace.publication_pr_status = Some("closed".to_string());
    let error = mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "closed",
    )
    .await
    .expect_err("a merged cycle must not be rewritten as closed-unmerged");
    assert!(error.to_string().contains("already terminal"));
}

#[tokio::test]
async fn merged_strict_cycle_rolls_over_on_exact_branch_without_clearing_early() {
    let (_temp, repo, project, base) = init_repo();
    let state = AppState::new_test();
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let workspace = strict_workspace(&project, &base).await;
    let worktree = Path::new(&workspace.worktree_path);
    std::fs::write(worktree.join("ticket.txt"), "ticket work\n").unwrap();
    git(worktree, &["add", "ticket.txt"]);
    git(worktree, &["commit", "-m", "ENG-42 - implement ticket"]);
    git(worktree, &["push", "origin", BRANCH]);
    git(&repo, &["merge", "--no-ff", BRANCH, "-m", "merge ticket"]);
    git(&repo, &["push", "origin", "main"]);
    mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .unwrap();

    let updated = rollover_strict_ticket_workspace(
        &state,
        &project,
        &workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
    .expect("safe merged cycle should roll over")
    .expect("strict rollover");

    assert_eq!(updated.branch_name, BRANCH);
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_pr_status, None);
    assert_eq!(updated.publication_push_status, None);
    assert!(Path::new(&updated.worktree_path).exists());
    let current = git(
        Path::new(&updated.worktree_path),
        &["branch", "--show-current"],
    );
    assert_eq!(current, BRANCH);
    let binding = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&project.id, BRANCH)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.cycle.generation, 2);
    assert_eq!(binding.cycle.state, TicketCanonicalBranchCycleState::Active);
    assert_eq!(binding.cycle.base_commit, updated.base_commit);
}

#[tokio::test]
async fn activated_rollover_retry_reuses_same_generation_and_clears_stale_workspace_metadata() {
    let (_temp, repo, project, base) = init_repo();
    let state = AppState::new_test();
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let workspace = strict_workspace(&project, &base).await;
    let worktree = Path::new(&workspace.worktree_path);
    std::fs::write(worktree.join("ticket.txt"), "ticket work\n").unwrap();
    git(worktree, &["add", "ticket.txt"]);
    git(worktree, &["commit", "-m", "ENG-42 - implement ticket"]);
    git(worktree, &["push", "origin", BRANCH]);
    git(&repo, &["merge", "--no-ff", BRANCH, "-m", "merge ticket"]);
    git(&repo, &["push", "origin", "main"]);
    mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .unwrap();

    let first = rollover_strict_ticket_workspace(
        &state,
        &project,
        &workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
    .expect("initial rollover should activate")
    .expect("strict rollover");
    assert_eq!(first.publication_pr_number, None);

    let retried = rollover_strict_ticket_workspace(
        &state,
        &project,
        &workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
    .expect("stale workspace retry should reconcile")
    .expect("strict rollover retry");
    let binding = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&project.id, BRANCH)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(binding.cycle.generation, 2);
    assert_eq!(binding.cycle.state, TicketCanonicalBranchCycleState::Active);
    assert_eq!(retried.publication_pr_number, None);
    assert_eq!(retried.base_commit, binding.cycle.base_commit);
}

#[tokio::test]
async fn squash_merged_strict_cycle_reuses_exact_branch_by_content_equivalence() {
    let (_temp, repo, project, base) = init_repo();
    let state = AppState::new_test();
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let workspace = strict_workspace(&project, &base).await;
    let worktree = Path::new(&workspace.worktree_path);
    std::fs::write(worktree.join("squashed.txt"), "same content\n").unwrap();
    git(worktree, &["add", "squashed.txt"]);
    git(
        worktree,
        &["commit", "-m", "ENG-42 - squash-compatible work"],
    );
    git(worktree, &["push", "origin", BRANCH]);
    git(&repo, &["merge", "--squash", BRANCH]);
    git(&repo, &["commit", "-m", "squash ticket"]);
    git(&repo, &["push", "origin", "main"]);
    mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .unwrap();

    let updated = rollover_strict_ticket_workspace(
        &state,
        &project,
        &workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
    .expect("content-equivalent squash merge should be reusable")
    .expect("strict rollover");

    assert_eq!(updated.branch_name, BRANCH);
    git(&repo, &["diff", "--quiet", "main", BRANCH]);
}

#[tokio::test]
async fn closed_unmerged_or_dirty_strict_cycle_preserves_workspace_and_publication_state() {
    let (_temp, _repo, project, base) = init_repo();
    let state = AppState::new_test();
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let mut workspace = strict_workspace(&project, &base).await;
    workspace.publication_pr_status = Some("closed".to_string());
    mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "closed",
    )
    .await
    .unwrap();

    let error = rollover_strict_ticket_workspace(
        &state,
        &project,
        &workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
    .expect_err("closed-unmerged cycle must block reuse");
    assert!(error.to_string().contains("closed without merge"));
    assert_eq!(workspace.publication_pr_number, Some(42));
    assert!(Path::new(&workspace.worktree_path).exists());
}

#[tokio::test]
async fn dirty_merged_strict_cycle_blocks_before_workspace_or_publication_mutation() {
    let (_temp, _repo, project, base) = init_repo();
    let state = AppState::new_test();
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let workspace = strict_workspace(&project, &base).await;
    std::fs::write(
        Path::new(&workspace.worktree_path).join("dirty.txt"),
        "uncommitted\n",
    )
    .unwrap();
    mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .unwrap();

    let error = rollover_strict_ticket_workspace(
        &state,
        &project,
        &workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
    .expect_err("dirty terminal workspace must block reuse");

    assert!(error.to_string().contains("uncommitted changes"));
    assert!(Path::new(&workspace.worktree_path).exists());
    assert_eq!(workspace.publication_pr_number, Some(42));
}

#[tokio::test]
async fn remote_only_ticket_history_blocks_reuse_before_clean_worktree_removal() {
    let (temp, repo, project, base) = init_repo();
    let state = AppState::new_test();
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let workspace = strict_workspace(&project, &base).await;
    let worktree = Path::new(&workspace.worktree_path);
    std::fs::write(worktree.join("ticket.txt"), "merged work\n").unwrap();
    git(worktree, &["add", "ticket.txt"]);
    git(worktree, &["commit", "-m", "ENG-42 - merged work"]);
    git(worktree, &["push", "origin", BRANCH]);
    git(&repo, &["merge", "--no-ff", BRANCH, "-m", "merge ticket"]);
    git(&repo, &["push", "origin", "main"]);

    let competing = temp.path().join("competing");
    let origin = temp.path().join("origin.git");
    git(
        temp.path(),
        &[
            "clone",
            origin.to_str().unwrap(),
            competing.to_str().unwrap(),
        ],
    );
    git(&competing, &["config", "user.email", "test@example.com"]);
    git(&competing, &["config", "user.name", "Remote Test"]);
    git(&competing, &["switch", BRANCH]);
    std::fs::write(competing.join("remote-only.txt"), "must not discard\n").unwrap();
    git(&competing, &["add", "remote-only.txt"]);
    git(&competing, &["commit", "-m", "ENG-42 - remote-only work"]);
    git(&competing, &["push", "origin", BRANCH]);
    mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .unwrap();

    let error = rollover_strict_ticket_workspace(
        &state,
        &project,
        &workspace,
        AgentConversationWorkspaceSetupMode::Blocking,
    )
    .await
    .expect_err("remote-only history must block reuse");

    assert!(error.to_string().contains("remote-only commits"));
    assert!(worktree.exists());
}

#[tokio::test]
async fn later_conversation_prepares_next_generation_only_after_clean_terminal_release() {
    let (_temp, repo, project, base) = init_repo();
    let state = AppState::new_test();
    state.project_repo.create(project.clone()).await.unwrap();
    let binding = state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(&project, &base))
        .await
        .unwrap();
    let workspace = strict_workspace(&project, &base).await;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let worktree = Path::new(&workspace.worktree_path);
    std::fs::write(worktree.join("ticket.txt"), "ticket work\n").unwrap();
    git(worktree, &["add", "ticket.txt"]);
    git(worktree, &["commit", "-m", "ENG-42 - implement ticket"]);
    git(worktree, &["push", "origin", BRANCH]);
    git(&repo, &["merge", "--no-ff", BRANCH, "-m", "merge ticket"]);
    git(&repo, &["push", "origin", "main"]);
    let terminal = mark_strict_ticket_cycle_terminal(
        state.ticket_canonical_branch_repo.as_ref(),
        &workspace,
        "merged",
    )
    .await
    .unwrap()
    .unwrap();

    let prepared = prepare_merged_strict_ticket_cycle_for_start(&state, &terminal)
        .await
        .expect("clean merged cycle should prepare for a later conversation");

    assert_eq!(prepared.branch_name, binding.branch_name);
    assert_eq!(prepared.cycle.generation, 2);
    assert_eq!(
        prepared.cycle.state,
        TicketCanonicalBranchCycleState::Preparing
    );
    assert!(!Path::new(&workspace.worktree_path).exists());
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_local_cleanup_status(&workspace.conversation_id)
            .await
            .unwrap()
            .as_deref(),
        Some("cleaned")
    );
}
