use super::agent_conversation_workspace::*;
use crate::application::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, Project,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::error::AppResult;
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_CHAT_PROJECT, AGENT_PERSONA_EXTRACTOR, AGENT_TASK_MANAGER,
};
use crate::infrastructure::memory::MemoryPlanBranchRepository;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn agent_name_maps_resolve_persona_builder_to_extractor() {
    assert_eq!(
        agent_name_for_workspace_mode(AgentConversationWorkspaceMode::PersonaBuilder),
        AGENT_PERSONA_EXTRACTOR
    );
}

#[test]
fn supervised_modes_route_to_their_canonical_agents() {
    assert_eq!(
        agent_name_for_workspace_mode(AgentConversationWorkspaceMode::Tasks),
        AGENT_TASK_MANAGER,
    );
    assert_eq!(
        agent_name_for_workspace_mode(AgentConversationWorkspaceMode::Autopilot),
        AGENT_CHAT_PROJECT,
    );
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo(root: &Path) {
    std::fs::create_dir_all(root).expect("repo root should be created");
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    std::fs::write(root.join("README.md"), "hello\n").expect("fixture file should be written");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

async fn prepare_isolated_local_branch(
    repo_path: &Path,
    worktree_parent: &Path,
    base: &str,
    conversation_id: &str,
    prefer_advanced_origin_base: bool,
) -> AppResult<AgentConversationWorkspace> {
    let mut project = Project::new("B3".to_string(), repo_path.to_string_lossy().to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
    prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Automation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Isolated),
            base_ref: Some(base.to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Blocking,
        AgentConversationWorkspacePrAutomationDefaults::default(),
        prefer_advanced_origin_base,
    )
    .await
}

#[tokio::test]
async fn automation_successor_worktree_cut_from_advanced_origin_base() {
    // B3: after run 1's PR merges, origin/<base> is ahead of the stale local
    // automation branch. The successor worktree must be cut from the advanced
    // remote-tracking ref, and the local branch must NOT be force-updated.
    let temp = tempfile::tempdir().unwrap();
    let origin = temp.path().join("origin.git");
    let repo = temp.path().join("repo");
    let helper = temp.path().join("helper");
    let worktrees = temp.path().join("worktrees");

    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "-b", "main"]);

    setup_repo(&repo);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "origin", "main"]);
    let base = "ralphx/ralphx/automation-adv";
    git(&repo, &["branch", base]);
    git(&repo, &["push", "origin", base]);
    let local_base_sha = git(&repo, &["rev-parse", base]);

    git(
        temp.path(),
        &["clone", origin.to_str().unwrap(), helper.to_str().unwrap()],
    );
    git(&helper, &["config", "user.email", "test@example.com"]);
    git(&helper, &["config", "user.name", "Test User"]);
    git(&helper, &["checkout", base]);
    std::fs::write(helper.join("merged.txt"), "merged\n").unwrap();
    git(&helper, &["add", "."]);
    git(&helper, &["commit", "-m", "merged run 1"]);
    git(&helper, &["push", "origin", base]);
    let merged_sha = git(&helper, &["rev-parse", base]);
    assert_ne!(local_base_sha, merged_sha);

    let workspace = prepare_isolated_local_branch(&repo, &worktrees, base, "conv-adv", true)
        .await
        .expect("workspace prepared");

    assert_eq!(
        workspace.base_ref, base,
        "stored base_ref stays the plain branch name for the run's own PR base"
    );
    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(merged_sha.as_str()),
        "successor worktree should base on the advanced origin tip"
    );
    assert_eq!(
        git(&repo, &["rev-parse", base]),
        local_base_sha,
        "local automation branch must never be force-updated"
    );
}

#[tokio::test]
async fn automation_successor_bases_on_current_tip_when_origin_unadvanced() {
    // B7: the common in-flight case — origin/<base> present but not yet
    // advanced. The fetch is a no-op and the run bases on the current tip.
    let temp = tempfile::tempdir().unwrap();
    let origin = temp.path().join("origin.git");
    let repo = temp.path().join("repo");
    let worktrees = temp.path().join("worktrees");

    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "-b", "main"]);
    setup_repo(&repo);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "origin", "main"]);
    let base = "ralphx/ralphx/automation-inflight";
    git(&repo, &["branch", base]);
    git(&repo, &["push", "origin", base]);
    let base_sha = git(&repo, &["rev-parse", base]);

    let workspace = prepare_isolated_local_branch(&repo, &worktrees, base, "conv-inflight", true)
        .await
        .expect("workspace prepared");

    assert_eq!(workspace.base_ref, base);
    assert_eq!(workspace.base_commit.as_deref(), Some(base_sha.as_str()));
}

#[tokio::test]
async fn automation_run_falls_back_to_local_base_when_origin_absent() {
    // B3: run 1 (base never published) — origin/<base> is absent, so the run
    // falls back to the local base without crashing.
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let worktrees = temp.path().join("worktrees");
    setup_repo(&repo);
    let base = "ralphx/ralphx/automation-run1";
    git(&repo, &["branch", base]);
    let local_base_sha = git(&repo, &["rev-parse", base]);

    let workspace = prepare_isolated_local_branch(&repo, &worktrees, base, "conv-run1", true)
        .await
        .expect("workspace prepared without origin");

    assert_eq!(workspace.base_ref, base);
    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(local_base_sha.as_str())
    );
}

#[tokio::test]
async fn automation_run_proceeds_on_local_base_when_fetch_fails() {
    // B3: a fetch failure (unreachable origin) must not block the run.
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let worktrees = temp.path().join("worktrees");
    setup_repo(&repo);
    git(
        &repo,
        &["remote", "add", "origin", "/nonexistent/origin.git"],
    );
    let base = "ralphx/ralphx/automation-fetchfail";
    git(&repo, &["branch", base]);
    let local_base_sha = git(&repo, &["rev-parse", base]);

    let workspace = prepare_isolated_local_branch(&repo, &worktrees, base, "conv-fetchfail", true)
        .await
        .expect("workspace prepared despite fetch failure");

    assert_eq!(workspace.base_ref, base);
    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(local_base_sha.as_str())
    );
}

#[tokio::test]
async fn non_automation_local_branch_ignores_advanced_origin_base() {
    // Scope: with prefer_advanced_origin_base = false, an advanced origin/<base>
    // is ignored and the worktree is cut from the local branch as before.
    let temp = tempfile::tempdir().unwrap();
    let origin = temp.path().join("origin.git");
    let repo = temp.path().join("repo");
    let helper = temp.path().join("helper");
    let worktrees = temp.path().join("worktrees");

    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "-b", "main"]);
    setup_repo(&repo);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "origin", "main"]);
    let base = "feature/local-scope";
    git(&repo, &["branch", base]);
    git(&repo, &["push", "origin", base]);
    let local_base_sha = git(&repo, &["rev-parse", base]);

    git(
        temp.path(),
        &["clone", origin.to_str().unwrap(), helper.to_str().unwrap()],
    );
    git(&helper, &["config", "user.email", "test@example.com"]);
    git(&helper, &["config", "user.name", "Test User"]);
    git(&helper, &["checkout", base]);
    std::fs::write(helper.join("merged.txt"), "merged\n").unwrap();
    git(&helper, &["add", "."]);
    git(&helper, &["commit", "-m", "advanced"]);
    git(&helper, &["push", "origin", base]);

    let workspace = prepare_isolated_local_branch(&repo, &worktrees, base, "conv-scope", false)
        .await
        .expect("workspace prepared");

    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(local_base_sha.as_str()),
        "non-automation workspace should ignore the advanced origin ref"
    );
}

#[tokio::test]
async fn linked_plan_branch_worktree_refuses_primary_checkout() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/primary-plan-branch";
    git(&repo_path, &["checkout", "-b", branch_name]);

    let mut project = Project::new(
        "Primary Plan Checkout".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-primary-plan-branch"),
        IdeationSessionId::from_string("session-primary-plan-branch"),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );

    let error = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .expect_err("primary checkout plan branch should be refused");

    assert!(error
        .to_string()
        .contains("refusing to publish from the primary checkout"));
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), branch_name);
}

#[tokio::test]
async fn linked_plan_branch_worktree_reuses_existing_isolated_checkout() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/existing-plan-worktree";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        "Existing Plan Checkout".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-existing-plan-branch"),
        IdeationSessionId::from_string("session-existing-plan-branch"),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    let workspace_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked plan worktree path should resolve");
    std::fs::create_dir_all(workspace_path.parent().expect("workspace path should nest"))
        .expect("workspace parent should be created");
    let workspace_path_arg = workspace_path.to_string_lossy().to_string();
    git(
        &repo_path,
        &["worktree", "add", workspace_path_arg.as_str(), branch_name],
    );

    let resolved = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .expect("existing linked plan worktree should be reused");

    assert_eq!(resolved, workspace_path);
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
    assert_eq!(git(&resolved, &["branch", "--show-current"]), branch_name);
}

#[tokio::test]
async fn effective_workspace_path_resolves_linked_plan_branch_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/effective-plan-worktree";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        "Effective Plan Checkout".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id =
        ChatConversationId::from_string("conversation-effective-plan-worktree".to_string());
    let session_id = IdeationSessionId::from_string("session-effective-plan-worktree");
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-effective-plan-branch"),
        session_id.clone(),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    let stale_direct_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("direct path should resolve");
    assert!(!stale_direct_path.exists());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        branch_name.to_string(),
        stale_direct_path.to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let plan_branch_repo = MemoryPlanBranchRepository::new();
    plan_branch_repo
        .create(plan_branch.clone())
        .await
        .expect("plan branch should be seeded");

    let resolved = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect("effective linked plan path should resolve");

    assert_eq!(
        resolved.path,
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("linked plan branch path should resolve")
    );
    assert_eq!(resolved.branch_name, branch_name);
    assert_eq!(
        git(&resolved.path, &["branch", "--show-current"]),
        branch_name
    );
}

#[tokio::test]
async fn effective_workspace_path_resolves_direct_workspace_path() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let mut project = Project::new(
        "Direct Effective Checkout".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id =
        ChatConversationId::from_string("conversation-effective-direct".to_string());
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    let plan_branch_repo = MemoryPlanBranchRepository::new();

    let resolved = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect("direct effective path should resolve");

    assert_eq!(resolved.path, PathBuf::from(&workspace.worktree_path));
    assert_eq!(resolved.branch_name, workspace.branch_name);
}

#[tokio::test]
async fn effective_workspace_path_rejects_workspace_project_mismatch() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    setup_repo(&repo_path);
    let project = Project::new(
        "Workspace Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let other_project = Project::new(
        "Other Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-project-mismatch".to_string()),
        other_project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/ralphx/agent-project-mismatch".to_string(),
        temp.path().join("missing").to_string_lossy().to_string(),
    );
    let plan_branch_repo = MemoryPlanBranchRepository::new();

    let error = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect_err("workspace project mismatch should be rejected");

    assert!(error.to_string().contains("belongs to project"));
}

#[tokio::test]
async fn effective_workspace_path_rejects_missing_linked_plan_branch() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    setup_repo(&repo_path);
    let project = Project::new(
        "Missing Plan Branch".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let session_id = IdeationSessionId::from_string("session-missing-plan-branch");
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-missing-plan-branch"),
        session_id.clone(),
        project.id.clone(),
        "feature/missing-plan-branch".to_string(),
        "main".to_string(),
    );
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-missing-plan-branch".to_string()),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        plan_branch.branch_name.clone(),
        temp.path().join("missing").to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id);
    let plan_branch_repo = MemoryPlanBranchRepository::new();

    let error = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect_err("missing linked plan branch should be rejected");

    assert!(error.to_string().contains("Linked plan branch not found"));
}

#[tokio::test]
async fn effective_workspace_path_rejects_linked_plan_project_mismatch() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    setup_repo(&repo_path);
    let project = Project::new(
        "Linked Plan Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let other_project = Project::new(
        "Other Linked Plan Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let session_id = IdeationSessionId::from_string("session-plan-project-mismatch");
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-project-mismatch"),
        session_id.clone(),
        other_project.id.clone(),
        "feature/plan-project-mismatch".to_string(),
        "main".to_string(),
    );
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-plan-project-mismatch".to_string()),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        plan_branch.branch_name.clone(),
        temp.path().join("missing").to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let plan_branch_repo = MemoryPlanBranchRepository::new();
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be seeded");

    let error = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect_err("linked plan project mismatch should be rejected");

    assert!(error.to_string().contains("belongs to project"));
}

#[tokio::test]
async fn effective_workspace_path_rejects_linked_plan_session_mismatch() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    setup_repo(&repo_path);
    let project = Project::new(
        "Linked Plan Session".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-session-mismatch"),
        IdeationSessionId::from_string("session-plan-branch"),
        project.id.clone(),
        "feature/plan-session-mismatch".to_string(),
        "main".to_string(),
    );
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-plan-session-mismatch".to_string()),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        plan_branch.branch_name.clone(),
        temp.path().join("missing").to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id =
        Some(IdeationSessionId::from_string("session-workspace"));
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let plan_branch_repo = MemoryPlanBranchRepository::new();
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be seeded");

    let error = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect_err("linked plan session mismatch should be rejected");

    assert!(error.to_string().contains("belongs to ideation session"));
}

#[tokio::test]
async fn effective_workspace_path_rejects_linked_plan_branch_mismatch() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    setup_repo(&repo_path);
    let project = Project::new(
        "Linked Plan Branch".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let session_id = IdeationSessionId::from_string("session-plan-branch-mismatch");
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-branch-mismatch"),
        session_id.clone(),
        project.id.clone(),
        "feature/plan-branch-mismatch".to_string(),
        "main".to_string(),
    );
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-plan-branch-mismatch".to_string()),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "feature/workspace-branch-mismatch".to_string(),
        temp.path().join("missing").to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let plan_branch_repo = MemoryPlanBranchRepository::new();
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be seeded");

    let error = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect_err("linked plan branch mismatch should be rejected");

    assert!(error.to_string().contains("records branch"));
}

#[tokio::test]
async fn linked_plan_branch_worktree_refuses_file_at_expected_path() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/file-plan-worktree";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        "File Plan Checkout".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-file-plan-branch"),
        IdeationSessionId::from_string("session-file-plan-branch"),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    let workspace_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked plan worktree path should resolve");
    std::fs::create_dir_all(workspace_path.parent().expect("workspace path should nest"))
        .expect("workspace parent should be created");
    std::fs::write(&workspace_path, "not a directory\n")
        .expect("file should be written at expected worktree path");

    let error = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .expect_err("file at linked plan path should be refused");

    assert!(error.to_string().contains("exists but is not a directory"));
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn linked_plan_branch_worktree_refuses_other_existing_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/other-plan-worktree";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);
    let other_worktree_path = temp.path().join("other-plan-worktree");
    let other_worktree_arg = other_worktree_path.to_string_lossy().to_string();
    git(
        &repo_path,
        &["worktree", "add", other_worktree_arg.as_str(), branch_name],
    );

    let mut project = Project::new(
        "Other Plan Checkout".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-other-plan-branch"),
        IdeationSessionId::from_string("session-other-plan-branch"),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );

    let error = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .expect_err("other linked plan worktree should be refused");

    assert!(error.to_string().contains("already checked out at"));
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn prepare_agent_conversation_workspace_runs_project_worktree_setup() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Setup".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project.custom_analysis = Some(
            r#"[{"path": ".", "label": "Agent setup", "worktree_setup": ["touch .agent_setup_marker"]}]"#
                .to_string(),
        );

    let conversation_id = ChatConversationId::from_string("conversation-setup-test".to_string());
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");

    assert!(
        Path::new(&workspace.worktree_path)
            .join(".agent_setup_marker")
            .exists(),
        "agent conversation worktree should run project worktree_setup commands"
    );
    let captured_head = GitService::get_head_sha(Path::new(&workspace.worktree_path))
        .await
        .expect("workspace HEAD should resolve");
    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(captured_head.as_str()),
        "agent conversation workspace should always capture the immutable base commit"
    );
}

#[tokio::test]
async fn prepare_agent_conversation_workspace_deferred_setup_runs_in_background() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Deferred Setup".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project.custom_analysis = Some(
            r#"[{"path": ".", "label": "Agent setup", "worktree_setup": ["touch .agent_deferred_setup_marker"]}]"#
                .to_string(),
        );

    let conversation_id =
        ChatConversationId::from_string("conversation-deferred-setup-test".to_string());
    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("workspace should be prepared before deferred setup completes");

    let marker_path = Path::new(&workspace.worktree_path).join(".agent_deferred_setup_marker");
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if marker_path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("deferred setup should complete in the background");
}

#[tokio::test]
async fn linked_pr_workspace_checks_out_selected_branch_and_links_publication_pr() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/pr-work";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);
    let mut project = Project::new(
        "Linked PR Workspace".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("33333333-3333-4333-8333-333333333333");

    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(branch_name.to_string()),
            display_name: Some("PR #42: Linked PR".to_string()),
            source_pull_request: Some(AgentWorkspaceSourcePullRequest {
                number: 42,
                url: Some("https://example.test/pull/42".to_string()),
                title: Some("Linked PR".to_string()),
                head_ref_name: branch_name.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: None,
            }),
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("linked PR workspace should prepare");

    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Linked
    );
    assert_eq!(workspace.branch_name, branch_name);
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::ProjectDefault
    );
    assert_eq!(workspace.base_ref, "main");
    assert_eq!(workspace.publication_pr_number, Some(42));
    assert_eq!(
        workspace.publication_pr_url.as_deref(),
        Some("https://example.test/pull/42")
    );
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("open"));
    let checked_out = git(
        Path::new(&workspace.worktree_path),
        &["branch", "--show-current"],
    );
    assert_eq!(checked_out, branch_name);
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn pr_workspace_omitted_branch_mode_defaults_to_isolated() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/pr-default-isolated";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);
    let mut project = Project::new(
        "Default PR Isolated Workspace".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("36363636-3636-4636-8636-363636363636");

    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some(branch_name.to_string()),
            display_name: Some("PR #42: Default isolated".to_string()),
            source_pull_request: Some(AgentWorkspaceSourcePullRequest {
                number: 42,
                url: Some("https://example.test/pull/42".to_string()),
                title: Some("Default isolated".to_string()),
                head_ref_name: branch_name.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: None,
            }),
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("PR workspace should prepare");

    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::LocalBranch
    );
    assert_eq!(workspace.base_ref, branch_name);
    assert_ne!(workspace.branch_name, branch_name);
    assert!(workspace.branch_name.contains("/agent-"));
    assert_eq!(
        workspace
            .source_pull_request
            .as_ref()
            .map(|source| source.number),
        Some(42)
    );
    assert_eq!(workspace.publication_pr_number, None);
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn review_pr_workspace_forces_isolated_even_when_linked_requested() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/review-pr-isolated";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);
    let mut project = Project::new(
        "Review PR Isolated Workspace".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("37373737-3737-4737-8737-373737373737");

    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::ReviewPr,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(branch_name.to_string()),
            display_name: Some("PR #77: Review isolated".to_string()),
            source_pull_request: Some(AgentWorkspaceSourcePullRequest {
                number: 77,
                url: Some("https://example.test/pull/77".to_string()),
                title: Some("Review isolated".to_string()),
                head_ref_name: branch_name.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: None,
            }),
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("Review PR workspace should prepare");

    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::LocalBranch
    );
    assert_eq!(workspace.base_ref, branch_name);
    assert_ne!(workspace.branch_name, branch_name);
    assert_eq!(workspace.publication_pr_number, None);
    assert_eq!(
        workspace
            .source_pull_request
            .as_ref()
            .map(|source| source.number),
        Some(77)
    );
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn review_pr_workspace_without_source_pull_request_fails_closed() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/review-pr-missing-source";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);
    let mut project = Project::new(
        "Review PR Missing Source".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("38383838-3838-4838-8838-383838383838");

    let error = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::ReviewPr,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(branch_name.to_string()),
            display_name: Some("Local branch without PR".to_string()),
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect_err("Review PR without PR metadata should fail closed");

    assert!(
        error
            .to_string()
            .contains("Review PR mode requires a selected pull request"),
        "unexpected error: {error}"
    );
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn project_default_selection_remains_isolated_even_when_linked_requested() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let mut project = Project::new(
        "Default Isolated Workspace".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("44444444-4444-4444-8444-444444444444");

    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("default workspace should prepare");

    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_ne!(workspace.branch_name, "main");
    assert!(workspace.branch_name.contains("/agent-"));
    assert_eq!(workspace.base_ref, "main");
}

#[tokio::test]
async fn linked_branch_workspace_refuses_primary_checkout() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/primary-agent-branch";
    git(&repo_path, &["checkout", "-b", branch_name]);
    let mut project = Project::new(
        "Primary Linked Checkout".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("55555555-5555-4555-8555-555555555555");

    let error = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(branch_name.to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect_err("primary checkout should block linked branch workspace");

    assert!(error
        .to_string()
        .contains("checked out in the project root"));
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), branch_name);
}

#[tokio::test]
async fn prepare_agent_conversation_workspace_applies_pr_automation_defaults() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Defaults".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id = ChatConversationId::from_string("conversation-defaults-test".to_string());
    let workspace = prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
        AgentConversationWorkspacePrAutomationDefaults {
            autofix_enabled: true,
            auto_merge_desired: true,
        },
        false,
    )
    .await
    .expect("workspace should be prepared");

    assert!(workspace.pr_autofix_enabled);
    assert!(workspace.pr_auto_merge_desired);
}

#[tokio::test]
async fn prepare_review_pr_workspace_suppresses_pr_automation_defaults_but_plan_retains_them() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let review_branch = "feature/review-pr-defaults";
    git(&repo_path, &["checkout", "-b", review_branch]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        "Review PR Defaults".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let defaults = AgentConversationWorkspacePrAutomationDefaults {
        autofix_enabled: true,
        auto_merge_desired: true,
    };

    let review_workspace = prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
        &project,
        &ChatConversationId::from_string("conversation-review-defaults"),
        AgentConversationWorkspaceMode::ReviewPr,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some(review_branch.to_string()),
            display_name: Some("PR #77: Defaults".to_string()),
            source_pull_request: Some(AgentWorkspaceSourcePullRequest {
                number: 77,
                url: Some("https://example.test/pull/77".to_string()),
                title: Some("Defaults".to_string()),
                head_ref_name: review_branch.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some("review-head".to_string()),
            }),
        },
        AgentConversationWorkspaceSetupMode::Deferred,
        defaults,
        false,
    )
    .await
    .expect("Review PR workspace should be prepared");

    let plan_workspace = prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
        &project,
        &ChatConversationId::from_string("conversation-plan-defaults"),
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
        defaults,
        false,
    )
    .await
    .expect("Plan workspace should be prepared");

    assert!(!review_workspace.pr_autofix_enabled);
    assert!(!review_workspace.pr_auto_merge_desired);
    assert!(plan_workspace.pr_autofix_enabled);
    assert!(plan_workspace.pr_auto_merge_desired);
}

#[tokio::test]
async fn prepare_agent_conversation_workspace_defaults_to_current_branch_when_it_differs() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    git(&repo_path, &["checkout", "-b", "feature/current-work"]);

    let mut project = Project::new(
        "Agent Current Branch".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project.base_branch = Some("main".to_string());

    let conversation_id =
        ChatConversationId::from_string("conversation-current-default".to_string());
    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection::default(),
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("workspace should be prepared");

    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::CurrentBranch
    );
    assert_eq!(workspace.base_ref, "feature/current-work");
    assert_eq!(
        workspace.base_display_name.as_deref(),
        Some("Current branch (feature/current-work)")
    );
}

#[tokio::test]
async fn prepare_agent_conversation_workspace_uses_ticket_branch_name_hint() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Ticket Branch".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let workspace =
        prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: None,
                source_pull_request: None,
            },
            AgentConversationWorkspaceSetupMode::Deferred,
            AgentConversationWorkspacePrAutomationDefaults::default(),
            false,
            Some(AgentConversationWorkspaceBranchNameHint {
                provider: "jira".to_string(),
                ticket_token: "PROJ-123".to_string(),
            }),
        )
        .await
        .expect("workspace should be prepared");

    assert!(
        workspace
            .branch_name
            .starts_with("ralphx/agent-ticket-branch/agent-jira-PROJ-123-11111111"),
        "unexpected branch name: {}",
        workspace.branch_name
    );
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::ProjectDefault
    );
    assert_eq!(workspace.base_ref, "main");
    assert_eq!(
        workspace.base_display_name.as_deref(),
        Some("Project default (main)")
    );
}

#[tokio::test]
async fn prepare_agent_conversation_workspace_sanitizes_ticket_branch_name_hint() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Unsafe Ticket".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let workspace =
        prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: None,
                source_pull_request: None,
            },
            AgentConversationWorkspaceSetupMode::Deferred,
            AgentConversationWorkspacePrAutomationDefaults::default(),
            false,
            Some(AgentConversationWorkspaceBranchNameHint {
                provider: "clickup".to_string(),
                ticket_token: "CU/../42.lock".to_string(),
            }),
        )
        .await
        .expect("workspace should be prepared");

    assert!(
        workspace
            .branch_name
            .starts_with("ralphx/agent-unsafe-ticket/agent-clickup-CU-42-lock-22222222"),
        "unexpected branch name: {}",
        workspace.branch_name
    );
    assert!(
        GitService::check_ref_format(&repo_path, &workspace.branch_name)
            .await
            .expect("ref format check should run"),
        "generated ticket branch should be a valid git ref"
    );
}

#[tokio::test]
async fn send_path_resolver_uses_stored_workspace_without_branch_probe() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Fast Send".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id =
        ChatConversationId::from_string("conversation-fast-send-test".to_string());
    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("workspace should be prepared");

    let worktree_path = Path::new(&workspace.worktree_path);
    git(worktree_path, &["checkout", "-b", "manual-drift"]);

    let resolved = resolve_agent_conversation_workspace_path_for_send(&project, &workspace)
        .expect("foreground send should trust stored workspace metadata");
    assert_eq!(resolved, worktree_path);

    let strict_error = resolve_valid_agent_conversation_workspace_path(&project, &workspace)
        .await
        .expect_err("strict validation should still catch branch drift");
    assert!(
        strict_error.to_string().contains("checked out"),
        "unexpected strict validation error: {strict_error}"
    );
}

#[tokio::test]
async fn rollover_agent_conversation_workspace_creates_new_branch_after_terminal_pr() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Rollover".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id = ChatConversationId::from_string("conversation-rollover-test".to_string());
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    let old_branch = workspace.branch_name.clone();
    let old_worktree_path = workspace.worktree_path.clone();
    workspace.publication_pr_number = Some(91);
    workspace.publication_pr_url = Some("https://example.test/pr/91".to_string());
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.status = crate::domain::entities::AgentConversationWorkspaceStatus::Missing;

    let updated = rollover_agent_conversation_workspace(&project, &workspace)
        .await
        .expect("terminal published workspace should roll over");

    assert_eq!(updated.worktree_path, old_worktree_path);
    assert!(
        updated.branch_name.starts_with(&format!("{old_branch}-")),
        "continuation branch should extend the canonical workspace branch"
    );
    assert_ne!(updated.branch_name, old_branch);
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_pr_url, None);
    assert_eq!(updated.publication_pr_status, None);
    assert_eq!(updated.publication_push_status, None);
    assert_eq!(
        updated.status,
        crate::domain::entities::AgentConversationWorkspaceStatus::Active
    );
    let checked_out = GitService::get_current_branch(Path::new(&updated.worktree_path))
        .await
        .expect("rolled workspace branch should resolve");
    assert_eq!(checked_out, updated.branch_name);
}

#[tokio::test]
async fn rollover_agent_conversation_workspace_preserves_linked_branch_mode() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let linked_branch = "feature/linked-rollover";
    git(&repo_path, &["checkout", "-b", linked_branch]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        "Linked Rollover".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id =
        ChatConversationId::from_string("conversation-linked-rollover-test".to_string());
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(linked_branch.to_string()),
            display_name: Some("PR #92: Linked rollover".to_string()),
            source_pull_request: Some(AgentWorkspaceSourcePullRequest {
                number: 92,
                url: Some("https://example.test/pr/92".to_string()),
                title: Some("Linked rollover".to_string()),
                head_ref_name: linked_branch.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: None,
            }),
        },
    )
    .await
    .expect("linked workspace should be prepared");
    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Linked
    );
    workspace.publication_pr_status = Some("merged".to_string());

    let updated = rollover_agent_conversation_workspace(&project, &workspace)
        .await
        .expect("terminal linked workspace should roll over");

    assert_eq!(
        updated.branch_mode,
        AgentConversationWorkspaceBranchMode::Linked
    );
    assert_ne!(updated.branch_name, workspace.branch_name);
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_pr_url, None);
    assert_eq!(updated.publication_pr_status, None);
    let checked_out = GitService::get_current_branch(Path::new(&updated.worktree_path))
        .await
        .expect("rolled linked workspace branch should resolve");
    assert_eq!(checked_out, updated.branch_name);
}

#[tokio::test]
async fn rollover_agent_conversation_workspace_blocks_dirty_old_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Dirty Rollover".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id =
        ChatConversationId::from_string("conversation-dirty-rollover-test".to_string());
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    workspace.publication_pr_status = Some("merged".to_string());
    std::fs::write(
        Path::new(&workspace.worktree_path).join("dirty.txt"),
        "uncommitted\n",
    )
    .expect("dirty file should be written");

    let error = rollover_agent_conversation_workspace(&project, &workspace)
        .await
        .expect_err("dirty rollover should be blocked");

    assert!(
        error.to_string().contains("uncommitted changes"),
        "dirty workspace should produce a clear validation error: {error}"
    );
    let checked_out = GitService::get_current_branch(Path::new(&workspace.worktree_path))
        .await
        .expect("old workspace should remain checked out");
    assert_eq!(checked_out, workspace.branch_name);
}

#[tokio::test]
async fn rollover_agent_conversation_workspace_blocks_stale_base_before_deleting_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Unsafe Rollover".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    git(&repo_path, &["checkout", "--orphan", "unmerged-base"]);
    std::fs::write(repo_path.join("README.md"), "unmerged\n")
        .expect("fixture file should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "unmerged"]);
    let unmerged_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);

    let conversation_id =
        ChatConversationId::from_string("conversation-stale-rollover-test".to_string());
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    workspace.base_ref = "feature/deleted-base".to_string();
    workspace.base_display_name = Some("Current branch (feature/deleted-base)".to_string());
    workspace.base_commit = Some(unmerged_sha);
    workspace.publication_pr_status = Some("merged".to_string());
    let old_worktree_path = PathBuf::from(&workspace.worktree_path);

    let error = rollover_agent_conversation_workspace(&project, &workspace)
        .await
        .expect_err("unsafe stale base should block rollover");

    assert!(error
        .to_string()
        .contains("not contained in the default branch"));
    assert!(old_worktree_path.exists());
    let checked_out = GitService::get_current_branch(&old_worktree_path)
        .await
        .expect("old workspace should remain checked out");
    assert_eq!(checked_out, workspace.branch_name);
}

#[tokio::test]
async fn rollover_agent_conversation_workspace_blocks_retarget_when_old_head_not_contained() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Retarget Rollover".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let main_sha = git(&repo_path, &["rev-parse", "main"]);

    let conversation_id =
        ChatConversationId::from_string("conversation-retarget-rollover-test".to_string());
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");

    std::fs::write(
        Path::new(&workspace.worktree_path).join("unmerged.txt"),
        "not merged to main\n",
    )
    .expect("fixture file should be written");
    git(
        Path::new(&workspace.worktree_path),
        &["add", "unmerged.txt"],
    );
    git(
        Path::new(&workspace.worktree_path),
        &["commit", "-m", "workspace-only change"],
    );

    workspace.base_ref = "feature/deleted-base".to_string();
    workspace.base_display_name = Some("Current branch (feature/deleted-base)".to_string());
    workspace.base_commit = Some(main_sha);
    workspace.publication_pr_status = Some("merged".to_string());
    let old_worktree_path = PathBuf::from(&workspace.worktree_path);

    let error = rollover_agent_conversation_workspace(&project, &workspace)
        .await
        .expect_err("old branch head outside default should block rollover");

    assert!(error
        .to_string()
        .contains("old workspace branch HEAD is not contained"));
    assert!(old_worktree_path.exists());
    let checked_out = GitService::get_current_branch(&old_worktree_path)
        .await
        .expect("old workspace should remain checked out");
    assert_eq!(checked_out, workspace.branch_name);
}

/// Proof obligation 6 (classification half): each fixture shape maps to exactly one resolution,
/// and `parent_root_present` separates a deleted workspace from a whole missing worktree root.
#[tokio::test]
async fn workspace_path_classification_distinguishes_missing_from_not_git_and_a_missing_root() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Classification".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id =
        ChatConversationId::from_string("conversation-classification-test".to_string());
    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("workspace should be prepared");
    let worktree_path = PathBuf::from(&workspace.worktree_path);

    assert_eq!(
        classify_agent_conversation_workspace_path(&project, &workspace).unwrap(),
        WorkspacePathResolution::Valid(worktree_path.clone())
    );

    // Directory present, `.git` gone.
    std::fs::remove_file(worktree_path.join(".git")).expect("worktree .git should be removable");
    assert_eq!(
        classify_agent_conversation_workspace_path(&project, &workspace).unwrap(),
        WorkspacePathResolution::NotGit(worktree_path.clone())
    );

    // Worktree deleted while the project's worktree root survives: a real orphan.
    std::fs::remove_dir_all(&worktree_path).expect("worktree should be removable");
    assert_eq!(
        classify_agent_conversation_workspace_path(&project, &workspace).unwrap(),
        WorkspacePathResolution::Missing {
            expected: worktree_path.clone(),
            parent_root_present: true,
        }
    );

    // Whole root gone: disk/mount trouble, never an orphan.
    let project_root = resolve_agent_conversation_project_workspace_dir(&project)
        .expect("project workspace dir should resolve");
    std::fs::remove_dir_all(&project_root).expect("project worktree root should be removable");
    assert_eq!(
        classify_agent_conversation_workspace_path(&project, &workspace).unwrap(),
        WorkspacePathResolution::Missing {
            expected: worktree_path.clone(),
            parent_root_present: false,
        }
    );
}

/// The typed classifier must not change any existing caller's error text.
#[tokio::test]
async fn the_untyped_resolver_still_produces_its_legacy_validation_strings() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Legacy Strings".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id =
        ChatConversationId::from_string("conversation-legacy-strings-test".to_string());
    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("workspace should be prepared");
    let worktree_path = PathBuf::from(&workspace.worktree_path);

    std::fs::remove_file(worktree_path.join(".git")).expect("worktree .git should be removable");
    let not_git = resolve_agent_conversation_workspace_path_for_send(&project, &workspace)
        .expect_err("a directory without .git must still fail");
    assert_eq!(
        not_git.to_string(),
        format!(
            "Validation error: Agent conversation workspace {} is not a git worktree: {}",
            workspace.conversation_id,
            worktree_path.display()
        )
    );

    std::fs::remove_dir_all(&worktree_path).expect("worktree should be removable");
    let missing = resolve_agent_conversation_workspace_path_for_send(&project, &workspace)
        .expect_err("a deleted worktree must still fail");
    assert_eq!(
        missing.to_string(),
        format!(
            "Validation error: Agent conversation workspace is missing: {}",
            worktree_path.display()
        )
    );
}

/// Proof obligation 8: a workspace linked to a plan branch resolves through
/// `ensure_linked_plan_branch_agent_worktree` and never evaluates its record path, so the
/// effective classifier must propagate the underlying error rather than report `Missing`.
#[tokio::test]
async fn the_effective_classifier_never_reports_missing_for_a_linked_plan_branch_workspace() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Agent Linked Plan".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let conversation_id =
        ChatConversationId::from_string("conversation-linked-plan-test".to_string());
    let mut workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("workspace should be prepared");

    // Record path deleted *and* linked to a plan branch the repo cannot find.
    std::fs::remove_dir_all(PathBuf::from(&workspace.worktree_path))
        .expect("worktree should be removable");
    workspace.linked_plan_branch_id = Some(crate::domain::entities::PlanBranchId::new());
    let plan_branch_repo = MemoryPlanBranchRepository::new();

    let error = classify_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        &plan_branch_repo,
    )
    .await
    .expect_err("an unresolvable plan branch must propagate, not classify the unused record path");
    assert!(
        error.to_string().contains("Linked plan branch not found"),
        "the plan-branch error must survive unchanged: {error}"
    );
}
