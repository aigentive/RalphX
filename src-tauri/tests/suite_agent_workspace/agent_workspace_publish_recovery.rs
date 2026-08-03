use std::process::Command;
use std::sync::Arc;

use ralphx_lib::application::agent_workspace_publish_recovery::{
    recover_stale_agent_workspace_publish_repairs, recover_stale_publish_repair_for_workspace,
    recover_stale_publish_repair_for_workspace_in_state,
};
use ralphx_lib::application::agent_workspace_review::load_agent_workspace_review_context;
use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use ralphx_lib::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairRepository,
};
use ralphx_lib::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
};

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

fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-1".to_string()),
        "ralphx/test/agent-workspace".to_string(),
        "/tmp/ralphx-agent-workspace".to_string(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("failed".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace
}

#[tokio::test]
async fn recovers_needs_agent_workspace_when_no_agent_run_is_active() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let workspace = needs_agent_workspace(conversation_id);
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");

    let run = agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed run");
    agent_run_repo
        .fail(&run.id, "repair agent exited")
        .await
        .expect("mark run failed");

    let recovered = recover_stale_agent_workspace_publish_repairs(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
    )
    .await
    .expect("recover stale repair");

    assert_eq!(recovered, 1);
    let refreshed = workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));

    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "stale_repair_recovered"
            && event.status == "succeeded"
            && event.classification.as_deref() == Some("stale_needs_agent")
    }));
}

#[tokio::test]
async fn keeps_needs_agent_workspace_locked_while_agent_run_is_active() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let workspace = needs_agent_workspace(conversation_id);
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed active run");

    let recovered = recover_stale_publish_repair_for_workspace(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        workspace,
    )
    .await
    .expect("check repair state");

    assert!(!recovered);
    let refreshed = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list events")
            .is_empty(),
        "active repairs must not be downgraded"
    );
}

#[tokio::test]
async fn preserves_pending_pr_fix_workspace_review_handoff_during_stale_recovery() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Pending Review Handoff".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");

    let conversation_id = ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    let mut workspace = needs_agent_workspace(conversation_id);
    workspace.project_id = project.id.clone();
    workspace.base_commit = Some(base_sha);
    workspace.worktree_path = repo.path().to_string_lossy().to_string();
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("reviewing".to_string());
    workspace.pr_supervision_summary =
        Some("PR fix completed; Workspace Review started before publishing resumes.".to_string());
    std::fs::write(repo.path().join("fix.txt"), "reviewed fix\n")
        .expect("write workspace review target");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    let review_context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("load review context");
    let review_target = review_context.target.expect("review target should exist");
    let mut monitor =
        AgentWorkspaceReviewMonitor::new(conversation_id, workspace.project_id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(review_target.scope);
    monitor.current_diff_fingerprint = Some(review_target.diff_fingerprint);
    monitor.workspace_head_sha = review_target.head_sha;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed running review monitor");
    let run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed run");
    state
        .agent_run_repo
        .fail(
            &run.id,
            "fixer exited after handing off to Workspace Review",
        )
        .await
        .expect("mark run failed");

    let refreshed = recover_stale_publish_repair_for_workspace_in_state(&state, workspace)
        .await
        .expect("check stale recovery");

    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent"),
        "stale recovery must not overwrite a pending Workspace Review publish handoff"
    );
    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(
        refreshed.pr_supervision_status.as_deref(),
        Some("reviewing")
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(
        !events
            .iter()
            .any(|event| event.step == "stale_repair_recovered"),
        "pending review handoff must not be converted to stale repair recovery"
    );
}

#[tokio::test]
async fn stale_passed_review_handoff_does_not_suppress_stale_recovery_cleanup() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Stale Passed Review Handoff".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");

    let conversation_id = ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
    let mut workspace = needs_agent_workspace(conversation_id);
    workspace.project_id = project.id.clone();
    workspace.base_commit = Some(base_sha);
    workspace.worktree_path = repo.path().to_string_lossy().to_string();
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("reviewing".to_string());
    workspace.pr_supervision_summary =
        Some("PR fix completed; Workspace Review started before publishing resumes.".to_string());
    let reviewed_path = repo.path().join("fix.txt");
    std::fs::write(&reviewed_path, "reviewed fix\n").expect("write reviewed change");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    let review_context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("load review context");
    let review_target = review_context.target.expect("review target should exist");
    std::fs::write(&reviewed_path, "changed after review\n").expect("stale review target");

    let mut monitor =
        AgentWorkspaceReviewMonitor::new(conversation_id, workspace.project_id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_outcome = ralphx_lib::domain::entities::AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ralphx_lib::domain::entities::ArtifactId::from_string(
        "stale-review-artifact",
    ));
    monitor.reviewed_target_scope = Some(review_target.scope);
    monitor.reviewed_head_sha = review_target.head_sha.clone();
    monitor.reviewed_diff_fingerprint = Some(review_target.diff_fingerprint.clone());
    monitor.current_target_scope = Some(review_target.scope);
    monitor.current_diff_fingerprint = Some(review_target.diff_fingerprint);
    monitor.workspace_head_sha = review_target.head_sha;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed stale passed review monitor");

    let run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed run");
    state
        .agent_run_repo
        .fail(
            &run.id,
            "fixer exited after handing off to Workspace Review",
        )
        .await
        .expect("mark run failed");

    let refreshed = recover_stale_publish_repair_for_workspace_in_state(&state, workspace)
        .await
        .expect("recover stale repair");

    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(events
        .iter()
        .any(|event| event.step == "pr_autofix_workspace_review_aborted"));
    assert!(
        !events
            .iter()
            .any(|event| event.step == "pr_autofix_workspace_review_passed"),
        "stale passed review must not close the handoff as publishable"
    );
}
