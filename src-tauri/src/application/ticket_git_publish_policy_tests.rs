use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;

use super::publish_resilience::PublishFailureClass;
use super::ticket_git_publish_policy::{
    install_ticket_git_commit_hook, load_ticket_git_publish_policy,
    refresh_ticket_git_publish_cycle_base, TicketGitPublishFailureKind,
};
use super::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    IdeationAnalysisBaseRefKind, ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycle,
    TicketCanonicalBranchCycleState, TicketGitConventionSnapshot,
};

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

fn init_repo() -> (tempfile::TempDir, PathBuf, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let origin = temp.path().join("origin.git");
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
    git(&repo, &["switch", "-c", "eng-42_ticket_ada"]);
    git(&repo, &["push", "-u", "origin", "eng-42_ticket_ada"]);
    (temp, repo, base)
}

fn strict_binding(project_id: ProjectId, base: &str) -> TicketCanonicalBranch {
    let mut binding = TicketCanonicalBranch::new_strict(
        project_id,
        "clickup",
        "ENG-42",
        "eng-42_ticket_ada",
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
    binding.cycle = TicketCanonicalBranchCycle {
        generation: 1,
        state: TicketCanonicalBranchCycleState::Active,
        base_commit: Some(base.to_string()),
        effective_merge_base: None,
        started_at: Some(Utc::now()),
        terminal_at: None,
    };
    binding
}

fn workspace(project_id: ProjectId, base: &str) -> AgentConversationWorkspace {
    let conversation = ChatConversation::new_project(project_id.clone());
    AgentConversationWorkspace::new(
        conversation.id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some(base.to_string()),
        "eng-42_ticket_ada".to_string(),
        "/ignored/hashed/worktree".to_string(),
    )
}

#[tokio::test]
async fn strict_publish_policy_validates_every_cycle_commit_and_returns_frozen_values() {
    let (_temp, repo, base) = init_repo();
    std::fs::write(repo.join("change.txt"), "valid\n").unwrap();
    git(&repo, &["add", "change.txt"]);
    git(&repo, &["commit", "-m", "ENG-42 - implement policy"]);
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-policy-valid".to_string());
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(project_id.clone(), &base))
        .await
        .unwrap();

    let workspace = workspace(project_id.clone(), &base);
    let mut policy =
        load_ticket_git_publish_policy(&state, &workspace, &repo, "conversation summary")
            .await
            .expect("valid strict range")
            .expect("strict policy");

    assert_eq!(policy.frozen_pr_title, "ENG-42 - Ticket");
    assert_eq!(
        policy.automatic_commit_subject,
        "ENG-42 - conversation summary"
    );
    assert_eq!(policy.validated_commit_count, 1);

    refresh_ticket_git_publish_cycle_base(&state, &workspace, &repo, &mut policy, &base)
        .await
        .expect("refreshed base should persist with active-cycle CAS");
    let stored = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&project_id, "eng-42_ticket_ada")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.cycle.effective_merge_base.as_deref(),
        Some(base.as_str())
    );
}

#[tokio::test]
async fn strict_publish_policy_rejects_bad_commit_with_typed_agent_fixable_failure() {
    let (_temp, repo, base) = init_repo();
    std::fs::write(repo.join("change.txt"), "invalid\n").unwrap();
    git(&repo, &["add", "change.txt"]);
    git(&repo, &["commit", "-m", "feat: bypass convention"]);
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-policy-invalid".to_string());
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(project_id.clone(), &base))
        .await
        .unwrap();

    let failure =
        load_ticket_git_publish_policy(&state, &workspace(project_id, &base), &repo, "summary")
            .await
            .expect_err("invalid commit must fail closed");

    assert_eq!(
        failure.kind,
        TicketGitPublishFailureKind::InvalidCommitSubjects
    );
    assert_eq!(failure.class(), PublishFailureClass::AgentFixable);
    assert_eq!(failure.offending_commits.len(), 1);
    assert_eq!(
        failure.offending_commits[0].subject,
        "feat: bypass convention"
    );
    assert!(!failure.offending_commits[0].short_sha.is_empty());
}

#[tokio::test]
async fn strict_publish_policy_rejects_checked_out_branch_drift_before_range_validation() {
    let (_temp, repo, base) = init_repo();
    git(&repo, &["switch", "main"]);
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-policy-branch".to_string());
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(project_id.clone(), &base))
        .await
        .unwrap();

    let failure =
        load_ticket_git_publish_policy(&state, &workspace(project_id, &base), &repo, "summary")
            .await
            .expect_err("branch drift must fail closed");

    assert_eq!(failure.kind, TicketGitPublishFailureKind::BranchMismatch);
    assert_eq!(failure.class(), PublishFailureClass::Operational);
    assert_eq!(failure.actual_branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn strict_publish_policy_classifies_remote_violation_as_operational() {
    let (_temp, repo, base) = init_repo();
    std::fs::write(repo.join("published.txt"), "published invalid\n").unwrap();
    git(&repo, &["add", "published.txt"]);
    git(&repo, &["commit", "-m", "feat: invalid remote history"]);
    git(&repo, &["push", "origin", "eng-42_ticket_ada"]);
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-policy-remote".to_string());
    state
        .ticket_canonical_branch_repo
        .create_if_absent(strict_binding(project_id.clone(), &base))
        .await
        .unwrap();

    let failure =
        load_ticket_git_publish_policy(&state, &workspace(project_id, &base), &repo, "summary")
            .await
            .expect_err("published violation must fail closed");

    assert_eq!(
        failure.kind,
        TicketGitPublishFailureKind::PublishedCommitSubjects
    );
    assert_eq!(failure.class(), PublishFailureClass::Operational);
    assert!(failure.offending_commits[0].published);
}

#[tokio::test]
async fn managed_commit_hook_preserves_existing_hook_and_rejects_bad_subjects() {
    let (_temp, repo, base) = init_repo();
    let prior_hooks = repo.join("prior-hooks");
    std::fs::create_dir_all(&prior_hooks).unwrap();
    std::fs::write(
        prior_hooks.join("commit-msg"),
        "#!/bin/sh\nprintf 'called\\n' >> \"$(git rev-parse --show-toplevel)/prior-called\"\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(prior_hooks.join("commit-msg"))
        .unwrap()
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    std::fs::set_permissions(prior_hooks.join("commit-msg"), permissions).unwrap();
    git(
        &repo,
        &["config", "core.hooksPath", prior_hooks.to_str().unwrap()],
    );
    let binding = strict_binding(
        ProjectId::from_string("project-policy-hook".to_string()),
        &base,
    );

    install_ticket_git_commit_hook(&repo, binding.strict_policy.as_ref().unwrap())
        .await
        .expect("managed hook should install");

    std::fs::write(repo.join("hook.txt"), "hook\n").unwrap();
    git(&repo, &["add", "hook.txt"]);
    let rejected = Command::new("git")
        .args(["commit", "-m", "wrong subject"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("ENG-42 - :summary:"));
    assert!(repo.join("prior-called").exists());

    git(&repo, &["commit", "-m", "ENG-42 - valid hook subject"]);
}
