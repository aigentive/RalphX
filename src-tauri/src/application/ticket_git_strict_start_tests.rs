use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::application::clickup_integration_service::ClickUpTaskContent;
use crate::application::external_issue_link_service::TicketConversationLinkInput;
use crate::application::ticket_git_strict_start::{
    activate_strict_ticket_branch_cycle, authoritative_clickup_task_for_conversation,
    ensure_strict_clickup_ticket_branch, preview_strict_clickup_ticket_branch,
    StrictClickUpTicketContext, StrictTicketGitBlockerCode,
};
use crate::application::AppState;
use crate::domain::entities::{
    Project, ProjectId, TicketCanonicalBranchCycleState, TicketCanonicalBranchPolicyKind,
};
use crate::domain::integrations::ClickUpIntegrationSettings;
use crate::tests::mock_github_service::MockGithubService;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let origin = temp.path().join("origin.git");
    assert!(Command::new("git")
        .args(["init", "--bare", origin.to_str().unwrap()])
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["init", "-b", "main", repo.to_str().unwrap()])
        .output()
        .unwrap()
        .status
        .success());
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "main\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    (temp, repo)
}

fn clickup_task(name: &str) -> ClickUpTaskContent {
    ClickUpTaskContent {
        id: "opaque-123".to_string(),
        custom_id: Some("ENG-42".to_string()),
        name: name.to_string(),
        url: Some("https://app.clickup.com/t/opaque-123".to_string()),
        description: String::new(),
        status_name: None,
        status_type: None,
        status_category: None,
        creator: None,
        assignees: Vec::new(),
        watchers: Vec::new(),
        tags: Vec::new(),
        comments: Vec::new(),
        attachments: Vec::new(),
        updated_at: None,
        space_id: None,
        list_name: None,
    }
}

fn strict_settings(enabled: bool) -> ClickUpIntegrationSettings {
    ClickUpIntegrationSettings {
        strict_git_naming_enabled: enabled,
        ..Default::default()
    }
}

async fn state_with_project(repo: &Path) -> (AppState, ProjectId, Arc<MockGithubService>) {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    let mut project = Project::new("Strict Ticket".to_string(), repo.display().to_string());
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();
    (state, project_id, github)
}

fn context<'a>(
    task: &'a ClickUpTaskContent,
    settings: &'a ClickUpIntegrationSettings,
) -> StrictClickUpTicketContext<'a> {
    StrictClickUpTicketContext {
        task,
        settings,
        username: Some("Ada Lovelace"),
        target_base_ref: "main",
    }
}

#[tokio::test]
async fn first_strict_start_freezes_and_pushes_the_exact_rendered_branch() {
    let (_temp, repo) = init_repo();
    let (state, project_id, github) = state_with_project(&repo).await;
    let task = clickup_task("Fix Login Race");
    let settings = strict_settings(true);

    let resolved =
        ensure_strict_clickup_ticket_branch(&state, &project_id, context(&task, &settings), None)
            .await
            .expect("strict branch should resolve")
            .expect("strict mode should be active");

    assert_eq!(
        resolved.binding.branch_name,
        "eng-42_fix-login-race_ada-lovelace"
    );
    assert_eq!(
        resolved.binding.policy_kind,
        TicketCanonicalBranchPolicyKind::StrictGitConvention
    );
    assert_eq!(
        resolved.binding.cycle.state,
        TicketCanonicalBranchCycleState::Preparing
    );
    let frozen = resolved.binding.strict_policy.as_ref().unwrap();
    assert_eq!(frozen.task_title, "Fix Login Race");
    assert_eq!(frozen.username.as_deref(), Some("Ada Lovelace"));
    assert_eq!(frozen.commit_subject_rule, "ENG-42 - Fix Login Race");
    assert_eq!(frozen.pr_title, "ENG-42 - Fix Login Race");
    assert!(resolved.binding.origin_pushed);
    assert_eq!(github.state().push_branch_calls, 1);
    assert_eq!(
        github.state().last_push_branch_name.as_deref(),
        Some("eng-42_fix-login-race_ada-lovelace")
    );
    assert_eq!(
        git_branch(&repo),
        "main",
        "strict branch must not be checked out in the project root"
    );
}

fn git_branch(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[tokio::test]
async fn persisted_strict_binding_wins_after_toggle_and_task_title_change() {
    let (_temp, repo) = init_repo();
    let (state, project_id, github) = state_with_project(&repo).await;
    let original = clickup_task("Original Title");
    let enabled = strict_settings(true);
    let first = ensure_strict_clickup_ticket_branch(
        &state,
        &project_id,
        context(&original, &enabled),
        None,
    )
    .await
    .unwrap()
    .unwrap();

    let renamed = clickup_task("Renamed Later");
    let disabled = strict_settings(false);
    let second = ensure_strict_clickup_ticket_branch(
        &state,
        &project_id,
        StrictClickUpTicketContext {
            task: &renamed,
            settings: &disabled,
            username: None,
            target_base_ref: "different-base-must-not-win",
        },
        None,
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(second.binding, first.binding);
    assert_eq!(github.state().push_branch_calls, 1);
}

#[tokio::test]
async fn read_only_preview_never_persists_or_creates_git_state() {
    let (_temp, repo) = init_repo();
    let (state, project_id, github) = state_with_project(&repo).await;
    let task = clickup_task("Preview Only");
    let settings = strict_settings(true);

    let preview =
        preview_strict_clickup_ticket_branch(&state, &project_id, context(&task, &settings))
            .await
            .expect("preview should render")
            .expect("strict preview");

    assert_eq!(preview.branch_name, "eng-42_preview-only_ada-lovelace");
    assert!(!preview.persisted);
    assert!(state
        .ticket_canonical_branch_repo
        .get(&project_id, "clickup", "ENG-42")
        .await
        .unwrap()
        .is_none());
    assert!(!crate::application::git_service::GitService::branch_exists(
        &repo,
        &preview.branch_name
    )
    .await
    .unwrap());
    assert_eq!(github.state().push_branch_calls, 0);
}

#[tokio::test]
async fn another_active_workspace_owner_returns_a_stable_blocker() {
    let (_temp, repo) = init_repo();
    let (state, project_id, _github) = state_with_project(&repo).await;
    let task = clickup_task("Owned Work");
    let settings = strict_settings(true);
    let first =
        ensure_strict_clickup_ticket_branch(&state, &project_id, context(&task, &settings), None)
            .await
            .unwrap()
            .unwrap();

    let conversation = crate::domain::entities::ChatConversation::new_project(project_id.clone());
    let mut workspace = crate::domain::entities::AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id.clone(),
        crate::domain::entities::AgentConversationWorkspaceMode::Edit,
        crate::domain::entities::IdeationAnalysisBaseRefKind::LocalBranch,
        "main".to_string(),
        Some("main".to_string()),
        first.binding.base_commit.clone(),
        first.binding.branch_name.clone(),
        repo.join("owned-worktree").display().to_string(),
    );
    workspace.branch_mode = crate::domain::entities::AgentConversationWorkspaceBranchMode::Linked;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let error =
        ensure_strict_clickup_ticket_branch(&state, &project_id, context(&task, &settings), None)
            .await
            .expect_err("second owner must be blocked");

    assert_eq!(error.code, StrictTicketGitBlockerCode::ActiveOwner);
    assert_eq!(
        error.owner_conversation_id.as_deref(),
        Some(conversation.id.as_str().as_str())
    );
}

#[tokio::test]
async fn mismatched_existing_ticket_branch_blocks_without_persisting_or_pushing() {
    let (_temp, repo) = init_repo();
    let (state, project_id, github) = state_with_project(&repo).await;
    git(&repo, &["branch", "feature/ENG-42-wrong", "main"]);
    let task = clickup_task("Expected Work");
    let settings = strict_settings(true);

    let error =
        ensure_strict_clickup_ticket_branch(&state, &project_id, context(&task, &settings), None)
            .await
            .expect_err("mismatched ticket evidence must block");

    assert_eq!(error.code, StrictTicketGitBlockerCode::EvidenceMismatch);
    assert!(state
        .ticket_canonical_branch_repo
        .get(&project_id, "clickup", "ENG-42")
        .await
        .unwrap()
        .is_none());
    assert_eq!(github.state().push_branch_calls, 0);
}

#[tokio::test]
async fn workspace_activation_is_generation_guarded_and_idempotent() {
    let (_temp, repo) = init_repo();
    let (state, project_id, _github) = state_with_project(&repo).await;
    let task = clickup_task("Activate Work");
    let settings = strict_settings(true);
    let prepared =
        ensure_strict_clickup_ticket_branch(&state, &project_id, context(&task, &settings), None)
            .await
            .unwrap()
            .unwrap();

    let active = activate_strict_ticket_branch_cycle(
        &state,
        &prepared.binding,
        prepared.binding.base_commit.as_deref(),
    )
    .await
    .expect("preparing cycle should activate");
    let repeated =
        activate_strict_ticket_branch_cycle(&state, &active, active.base_commit.as_deref())
            .await
            .expect("activation should be idempotent");

    assert_eq!(active.cycle.state, TicketCanonicalBranchCycleState::Active);
    assert_eq!(repeated.cycle, active.cycle);
}

async fn link_clickup_conversation(
    state: &AppState,
    project_id: &ProjectId,
    conversation_id: &crate::domain::entities::ChatConversationId,
) {
    state
        .external_issue_link_service
        .upsert_ticket_conversation_link(TicketConversationLinkInput {
            provider: "clickup".to_string(),
            external_kind: "clickup".to_string(),
            external_id: "opaque-123".to_string(),
            external_key: Some("ENG-42".to_string()),
            external_url: Some("https://app.clickup.com/t/opaque-123".to_string()),
            conversation_id: conversation_id.as_str(),
            project_id: project_id.to_string(),
            local_sha: None,
            local_state: Some("active".to_string()),
            metadata_json: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn disabled_mode_upgrade_ignores_unbound_clickup_link_without_api_lookup() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let conversation_id = crate::domain::entities::ChatConversationId::new();
    link_clickup_conversation(&state, &project_id, &conversation_id).await;

    let task = authoritative_clickup_task_for_conversation(&state, &project_id, &conversation_id)
        .await
        .expect("disabled strict mode should remain compatible");

    assert!(task.is_none());
}

#[tokio::test]
async fn existing_binding_recovers_mode_upgrade_from_frozen_link_without_live_api() {
    let (_temp, repo) = init_repo();
    let (state, project_id, _github) = state_with_project(&repo).await;
    let task = clickup_task("Frozen Upgrade Title");
    let settings = strict_settings(true);
    ensure_strict_clickup_ticket_branch(&state, &project_id, context(&task, &settings), None)
        .await
        .unwrap()
        .unwrap();
    let conversation_id = crate::domain::entities::ChatConversationId::new();
    link_clickup_conversation(&state, &project_id, &conversation_id).await;

    let recovered =
        authoritative_clickup_task_for_conversation(&state, &project_id, &conversation_id)
            .await
            .expect("frozen binding should not require live ClickUp")
            .expect("linked task should recover");

    assert_eq!(recovered.id, "opaque-123");
    assert_eq!(recovered.custom_id.as_deref(), Some("ENG-42"));
    assert_eq!(recovered.name, "Frozen Upgrade Title");
}
