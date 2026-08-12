use super::*;
use crate::application::agent_workspace_review_approval::approve_agent_workspace_review_anyway;
use crate::application::chat_service::MockChatService;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderSettings, AgenticClient, LogicalEffort, ManualRoleDefault,
    ManualRoleRuntimeOverride, ManualServiceTier, ProviderSessionRef, RoutingRole,
    CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
};
use crate::domain::entities::{
    AgentConversationJiraIssueLink, AgentConversationWorkspaceMode, AgentRun,
    AgentWorkspaceReviewApprovalSnapshot, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewOutcome, AgentWorkspaceSourcePullRequest, Artifact, ArtifactId,
    ArtifactType, ChatConversation, ChatConversationId, ChatMessage, ChatMessageId,
    ChatTimelineItem, ChatTimelineItemKind, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus, ProjectId, RuntimeSource,
    TaskId,
};
use crate::domain::repositories::{AgentProviderSettingsRepository, ReviewSettingsRepository};
use crate::domain::review::ReviewSettings;
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::infrastructure::MockAgenticClient;
use chrono::DateTime;
use std::collections::BTreeSet;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc as StdArc, Mutex as StdMutex};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

/// Deadlines generous enough that terminal-run waiter tests never trip a bound.
/// Tests that assert deadline behavior inject their own millisecond-scale values.
fn test_waiter_deadlines() -> WorkspaceReviewWaiterDeadlines {
    WorkspaceReviewWaiterDeadlines {
        idle_timeout: Duration::from_secs(60),
        max_wall_clock: Duration::from_secs(60),
        completion_grace: Duration::from_secs(1),
    }
}

#[derive(Clone, Debug)]
struct WorkspaceReviewTimingEvent {
    operation: String,
    phase: String,
    fields: BTreeSet<String>,
}

struct WorkspaceReviewTimingLayer {
    captured: StdArc<StdMutex<Vec<WorkspaceReviewTimingEvent>>>,
}

#[derive(Clone, Debug)]
struct WorkspaceReviewGitLaneEvent {
    command: String,
    lane: String,
}

struct WorkspaceReviewGitLaneLayer {
    captured: StdArc<StdMutex<Vec<WorkspaceReviewGitLaneEvent>>>,
}

struct WorkspaceReviewGitWriteTreeGateLayer {
    completed_write_trees: AtomicUsize,
    reached: std::sync::mpsc::SyncSender<()>,
    resume: StdMutex<std::sync::mpsc::Receiver<()>>,
}

impl<S: tracing::Subscriber> Layer<S> for WorkspaceReviewGitWriteTreeGateLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        #[derive(Default)]
        struct CommandVisitor {
            command: Option<String>,
        }

        impl tracing::field::Visit for CommandVisitor {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "command" {
                    self.command = Some(value.to_string());
                }
            }

            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "command" {
                    self.command = Some(format!("{value:?}").replace('"', ""));
                }
            }
        }

        let mut visitor = CommandVisitor::default();
        event.record(&mut visitor);
        if visitor.command.as_deref() != Some("write-tree") {
            return;
        }
        let completed = self.completed_write_trees.fetch_add(1, Ordering::SeqCst) + 1;
        if completed != 2 {
            return;
        }
        self.reached
            .send(())
            .expect("fingerprint test should wait for the final settle check");
        self.resume
            .lock()
            .expect("fingerprint resume receiver lock should remain available")
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("fingerprint test should resume the final settle check");
    }
}

impl<S: tracing::Subscriber> Layer<S> for WorkspaceReviewGitLaneLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        #[derive(Default)]
        struct GitLaneVisitor {
            command: Option<String>,
            lane: Option<String>,
        }

        impl tracing::field::Visit for GitLaneVisitor {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                match field.name() {
                    "command" => self.command = Some(value.to_string()),
                    "lane" => self.lane = Some(value.to_string()),
                    _ => {}
                }
            }

            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let value = format!("{value:?}").replace('"', "");
                match field.name() {
                    "command" => self.command = Some(value),
                    "lane" => self.lane = Some(value),
                    _ => {}
                }
            }
        }

        let mut visitor = GitLaneVisitor::default();
        event.record(&mut visitor);
        if let (Some(command), Some(lane)) = (visitor.command, visitor.lane) {
            self.captured
                .lock()
                .expect("git lane capture lock should remain available")
                .push(WorkspaceReviewGitLaneEvent { command, lane });
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for WorkspaceReviewTimingLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        #[derive(Default)]
        struct TimingVisitor {
            operation: Option<String>,
            phase: Option<String>,
            fields: BTreeSet<String>,
        }

        impl tracing::field::Visit for TimingVisitor {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                self.fields.insert(field.name().to_string());
                match field.name() {
                    "operation" => self.operation = Some(value.to_string()),
                    "phase" => self.phase = Some(value.to_string()),
                    _ => {}
                }
            }

            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.fields.insert(field.name().to_string());
                match field.name() {
                    "operation" => self.operation = Some(format!("{value:?}").replace('"', "")),
                    "phase" => self.phase = Some(format!("{value:?}").replace('"', "")),
                    _ => {}
                }
            }
        }

        let mut visitor = TimingVisitor::default();
        event.record(&mut visitor);
        let (Some(operation), Some(phase)) = (visitor.operation, visitor.phase) else {
            return;
        };
        if !operation.starts_with("workspace_review_") || !operation.ends_with("_phase") {
            return;
        }
        self.captured
            .lock()
            .expect("timing capture lock should remain available")
            .push(WorkspaceReviewTimingEvent {
                operation,
                phase,
                fields: visitor.fields,
            });
    }
}

fn capture_workspace_review_timings() -> (
    tracing::dispatcher::DefaultGuard,
    StdArc<StdMutex<Vec<WorkspaceReviewTimingEvent>>>,
) {
    let captured = StdArc::new(StdMutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(WorkspaceReviewTimingLayer {
        captured: StdArc::clone(&captured),
    });
    (subscriber.set_default(), captured)
}

fn assert_workspace_review_timing_phases(
    captured: &StdArc<StdMutex<Vec<WorkspaceReviewTimingEvent>>>,
    operation: &str,
    expected_phases: &[&str],
) {
    let captured = captured
        .lock()
        .expect("timing capture lock should remain available");
    for expected_phase in expected_phases {
        let event = captured
            .iter()
            .find(|event| event.operation == operation && event.phase == *expected_phase)
            .unwrap_or_else(|| {
                panic!("missing {operation} phase {expected_phase}; captured events: {captured:?}")
            });
        assert!(
            event.fields.contains("elapsed_ms"),
            "{operation}/{expected_phase} should record elapsed_ms"
        );
        assert!(
            event.fields.contains("total_elapsed_ms"),
            "{operation}/{expected_phase} should record total_elapsed_ms"
        );
    }
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

fn init_repo() -> (tempfile::TempDir, PathBuf, String) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir should be created");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_sha = git(&repo, &["rev-parse", "HEAD"]);
    (temp, repo, base_sha)
}

async fn seed_project(state: &AppState, repo: &Path) -> Project {
    let mut project = Project::new(
        "Workspace Review".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    project
}

fn workspace(
    project: &Project,
    worktree_path: &Path,
    base_kind: IdeationAnalysisBaseRefKind,
    base_ref: &str,
    base_commit: Option<String>,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::new(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        base_kind,
        base_ref.to_string(),
        Some(base_ref.to_string()),
        base_commit,
        "ralphx/test/workspace-review".to_string(),
        worktree_path.to_string_lossy().to_string(),
    )
}

fn committed_workspace_delta(repo: &Path) {
    std::fs::write(repo.join("committed.rs"), "pub fn committed() {}\n")
        .expect("committed file should be written");
    git(repo, &["add", "committed.rs"]);
    git(repo, &["commit", "-m", "committed change"]);
}

fn committed_workspace_delta_on_branch(repo: &Path, branch: &str) -> String {
    git(repo, &["checkout", "-b", branch]);
    std::fs::write(repo.join("committed.rs"), "pub fn committed() {}\n")
        .expect("committed file should be written");
    git(repo, &["add", "committed.rs"]);
    git(repo, &["commit", "-m", "committed change"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn commit_followup_change(repo: &Path) -> String {
    std::fs::write(repo.join("followup.rs"), "pub fn followup() {}\n")
        .expect("followup file should be written");
    git(repo, &["add", "followup.rs"]);
    git(repo, &["commit", "-m", "followup change"]);
    git(repo, &["rev-parse", "HEAD"])
}

async fn seed_conversation(state: &AppState, workspace: &AgentConversationWorkspace) {
    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.id = workspace.conversation_id.clone();
    conversation.agent_mode = Some(workspace.mode);
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
}

async fn persist_workspace(state: &AppState, workspace: &AgentConversationWorkspace) {
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
}

fn fixer_attempt_monitor(
    conversation_id: ChatConversationId,
    project_id: ProjectId,
    attempt_id: &str,
    status: &str,
) -> AgentWorkspaceReviewMonitor {
    let fingerprint = format!("diff-{attempt_id}");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some(fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(fingerprint);
    monitor.review_artifact_id = Some(ArtifactId::from_string(format!("artifact-{attempt_id}")));
    monitor.review_artifact_version = Some(1);
    monitor.review_requested_changes_artifact_id = Some(ArtifactId::from_string(format!(
        "requested-changes-{attempt_id}"
    )));
    monitor.review_requested_changes_artifact_version = Some(1);
    monitor.review_blocking_fingerprint = Some(format!("blocker-{attempt_id}"));
    monitor.review_fixer_status = Some(status.to_string());
    monitor.review_fixer_attempt_id = Some(attempt_id.to_string());
    monitor
}

struct FailingReviewSettingsRepository;

#[async_trait::async_trait]
impl ReviewSettingsRepository for FailingReviewSettingsRepository {
    async fn get_settings(&self) -> Result<ReviewSettings, Box<dyn std::error::Error>> {
        Err(Box::new(std::io::Error::other(
            "review settings are unavailable",
        )))
    }

    async fn update_settings(
        &self,
        _settings: &ReviewSettings,
    ) -> Result<ReviewSettings, Box<dyn std::error::Error>> {
        Err(Box::new(std::io::Error::other(
            "review settings are unavailable",
        )))
    }
}

async fn persist_active_review_for_current_target(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    run_id: &str,
    artifact_id: &str,
    cycle_count: i64,
) -> AgentWorkspaceReviewTarget {
    let context = load_agent_workspace_review_context(state, workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(run_id.to_string()),
        ArtifactId::from_string(artifact_id),
        1,
        Utc::now(),
        None,
    );
    monitor.review_fixer_cycle_count = cycle_count;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("active monitor should persist");
    target
}

async fn wait_for_monitor_status(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    status: AgentWorkspaceReviewMonitorStatus,
) -> AgentWorkspaceReviewMonitor {
    for _ in 0..100 {
        if let Some(monitor) = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor read should succeed")
        {
            if monitor.status == status {
                return monitor;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("monitor did not reach status {status}");
}

#[tokio::test]
async fn cleanup_before_plan_invalidates_inactive_pass_and_bypass_authority() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-authority-cleanup".to_string());

    for (suffix, outcome, with_bypass) in [
        ("passed", AgentWorkspaceReviewOutcome::Passed, false),
        ("bypassed", AgentWorkspaceReviewOutcome::Blocking, true),
    ] {
        let conversation_id =
            ChatConversationId::from_string(format!("plan-authority-cleanup-{suffix}"));
        let workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project_id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "main".to_string(),
            None,
            Some("base-sha".to_string()),
            format!("ralphx/test/plan-authority-cleanup-{suffix}"),
            format!("/tmp/ralphx-plan-authority-cleanup-{suffix}"),
        );
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let artifact_id = ArtifactId::from_string(format!("review-artifact-{suffix}"));
        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        monitor.review_outcome = outcome;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        monitor.review_artifact_id = Some(artifact_id.clone());
        monitor.review_artifact_version = Some(7);
        if with_bypass {
            monitor.review_gate_bypassed_at = Some(Utc::now());
            monitor.review_gate_bypassed_target_scope =
                Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
            monitor.review_gate_bypassed_diff_fingerprint = Some("fingerprint".to_string());
            monitor.review_gate_bypassed_artifact_id = Some(artifact_id.clone());
            monitor.review_gate_bypassed_artifact_version = Some(7);
        }
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("monitor should persist");

        cleanup_workspace_review_for_plan_boundary(&state, &workspace, None)
            .await
            .expect("inactive review authority should be invalidated without a runtime");

        let cleaned = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should remain as history");
        assert_eq!(cleaned.status, AgentWorkspaceReviewMonitorStatus::Blocked);
        assert_eq!(
            cleaned.review_outcome,
            AgentWorkspaceReviewOutcome::RunFailed
        );
        assert_eq!(
            cleaned.review_gate_status,
            AgentWorkspaceReviewGateStatus::Failed
        );
        assert_eq!(cleaned.review_artifact_id, Some(artifact_id));
        assert_eq!(cleaned.review_artifact_version, Some(7));
        assert!(cleaned.review_gate_bypassed_at.is_none());
        assert!(cleaned.review_gate_bypassed_artifact_id.is_none());
        assert_eq!(
            cleaned.last_error.as_deref(),
            Some(WORKSPACE_REVIEW_MODE_CHANGED_TO_PLAN_ERROR)
        );
    }
}

#[test]
fn inherited_reference_metadata_deduplicates_limits_and_ignores_invalid_payloads() {
    let mut inherited = WorkspaceReviewInheritedReferences::default();
    let mut project_seen = BTreeSet::new();
    let mut integration_seen = BTreeSet::new();
    let mut artifact_seen = BTreeSet::new();

    merge_workspace_review_references_from_metadata(
        None,
        &mut inherited,
        &mut project_seen,
        Some(&mut integration_seen),
        &mut artifact_seen,
    );
    merge_workspace_review_references_from_metadata(
        Some("not-json"),
        &mut inherited,
        &mut project_seen,
        Some(&mut integration_seen),
        &mut artifact_seen,
    );
    merge_workspace_review_references_from_metadata(
        Some("[]"),
        &mut inherited,
        &mut project_seen,
        Some(&mut integration_seen),
        &mut artifact_seen,
    );

    let metadata = serde_json::json!({
        "composer_project_references": [
            { "path": "README.md", "kind": "file" },
            { "path": "README.md", "kind": "file" },
            { "path": "src", "kind": "directory" },
            { "path": "docs", "kind": "directory" },
            { "path": "frontend", "kind": "directory" },
            { "path": "src-tauri", "kind": "directory" },
            { "path": "package.json", "kind": "file" },
            { "path": "Cargo.toml", "kind": "file" },
            { "path": "CLAUDE.md", "kind": "file" },
            { "path": "ignored-after-cap.md", "kind": "file" }
        ],
        "composer_integration_references": [
            {
                "provider": "atlassian",
                "kind": "jira",
                "id": "RX-42",
                "key": "RX-42",
                "title": "Fix Review gate"
            },
            {
                "provider": "atlassian",
                "kind": "jira",
                "id": "RX-42",
                "key": "DIFFERENT-DISPLAY-KEY",
                "title": "Duplicate"
            },
            { "provider": "linear", "kind": "issue", "id": "LIN-1" },
            { "provider": "clickup", "kind": "task", "id": "CU-1" },
            { "provider": "granola", "kind": "note", "id": "GN-1" },
            { "provider": "github", "kind": "issue", "id": "GH-1" },
            { "provider": "sentry", "kind": "issue", "id": "SEN-1" },
            { "provider": "notion", "kind": "page", "id": "NOT-1" },
            { "provider": "slack", "kind": "thread", "id": "SL-1" },
            { "provider": "ignored", "kind": "thread", "id": "IGN-1" }
        ],
        "composer_artifact_references": [
            { "artifactId": "artifact-1", "kind": "plan", "title": "Plan" },
            { "artifactId": "artifact-1", "kind": "plan", "title": "Duplicate" },
            { "artifactId": "artifact-2", "kind": "design" },
            { "artifactId": "artifact-3", "kind": "spec" },
            { "artifactId": "artifact-4", "kind": "notes" },
            { "artifactId": "artifact-5", "kind": "review" },
            { "artifactId": "artifact-6", "kind": "diff" },
            { "artifactId": "artifact-7", "kind": "trace" },
            { "artifactId": "artifact-8", "kind": "context" },
            { "artifactId": "artifact-9", "kind": "ignored" }
        ]
    })
    .to_string();

    merge_workspace_review_references_from_metadata(
        Some(&metadata),
        &mut inherited,
        &mut project_seen,
        Some(&mut integration_seen),
        &mut artifact_seen,
    );

    assert_eq!(inherited.project_references.len(), 8);
    assert_eq!(inherited.project_references[0].path, "README.md");
    assert_eq!(inherited.project_references[1].path, "src");
    assert!(!inherited
        .project_references
        .iter()
        .any(|reference| reference.path == "ignored-after-cap.md"));
    assert_eq!(inherited.integration_references.len(), 8);
    assert_eq!(
        inherited.integration_references[0].key.as_deref(),
        Some("RX-42")
    );
    assert!(!inherited
        .integration_references
        .iter()
        .any(|reference| reference.id == "IGN-1"));
    assert_eq!(inherited.artifact_references.len(), 8);
    assert_eq!(inherited.artifact_references[0].artifact_id, "artifact-1");
    assert!(!inherited
        .artifact_references
        .iter()
        .any(|reference| reference.artifact_id == "artifact-9"));

    merge_workspace_review_references_from_metadata(
        Some(&metadata),
        &mut inherited,
        &mut project_seen,
        Some(&mut integration_seen),
        &mut artifact_seen,
    );
    assert_eq!(inherited.project_references.len(), 8);
    assert_eq!(inherited.integration_references.len(), 8);
    assert_eq!(inherited.artifact_references.len(), 8);
}

#[tokio::test]
async fn linked_workspace_plan_reference_allows_no_link_but_rejects_broken_authority() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );

    assert!(load_linked_workspace_plan_snapshot(&state, &workspace)
        .await
        .expect("missing link should load")
        .is_none());

    workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string("missing-session"));
    let missing_session_error = load_linked_workspace_plan_snapshot(&state, &workspace)
        .await
        .expect_err("a present link to a missing session must fail closed");
    assert!(missing_session_error
        .to_string()
        .contains("no longer exists"));

    let empty_session = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .build();
    let empty_session = state
        .ideation_session_repo
        .create(empty_session)
        .await
        .expect("empty planning session should persist");
    workspace.linked_ideation_session_id = Some(empty_session.id.clone());
    assert!(load_linked_workspace_plan_snapshot(&state, &workspace)
        .await
        .expect("empty session should load")
        .is_none());

    let missing_artifact_id = ArtifactId::from_string("missing-plan-artifact");
    let missing_artifact_session = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .inherited_plan_artifact_id(missing_artifact_id.clone())
        .inherited_plan_blueprint_artifact_id(ArtifactId::from_string(
            "missing-plan-blueprint-artifact",
        ))
        .build();
    let missing_artifact_session = state
        .ideation_session_repo
        .create(missing_artifact_session)
        .await
        .expect("missing-artifact planning session should persist");
    workspace.linked_ideation_session_id = Some(missing_artifact_session.id.clone());

    let missing_artifact_error = load_linked_workspace_plan_snapshot(&state, &workspace)
        .await
        .expect_err("a linked missing plan artifact must fail closed");
    assert!(missing_artifact_error
        .to_string()
        .contains("no longer exists"));
}

#[test]
fn review_packet_handles_status_edges_limits_and_truncation() {
    let diff = "\
metadata before first file
diff --git a/modified.rs b/modified.rs
--- a/modified.rs
+++ b/modified.rs
@@
-old
+new
diff --git a/added.rs b/added.rs
new file mode 100644
--- /dev/null
+++ b/added.rs
@@
+added
diff --git a/deleted.rs b/deleted.rs
deleted file mode 100644
--- a/deleted.rs
+++ /dev/null
@@
-deleted
diff --git a/old_name.rs b/old_name.rs
similarity index 100%
rename from old_name.rs
rename to \"renamed file.rs\"
diff --git a/status_added.rs b/status_added.rs
--- a/status_added.rs
+++ b/status_added.rs
@@
+status added
";
    let large_diff = format!(
        "diff --git a/large.rs b/large.rs\n--- a/large.rs\n+++ b/large.rs\n@@\n+{}\n",
        "x".repeat(WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS + 64)
    );
    let mut status = String::from(
        "\
A  status_added.rs
 D status_deleted.rs
R  old_status.rs -> status_renamed.rs
 M status_modified.rs
?? untracked.rs
?? /dev/null
x
",
    );
    status.push_str("??    \n");
    for index in 0..=WORKSPACE_REVIEW_MAX_CHANGED_FILES {
        status.push_str(&format!("?? zz-overflow-{index:03}.rs\n"));
    }

    let packet = build_review_packet(
        &[
            ("edge diff", diff),
            ("empty diff", "   "),
            ("large diff", &large_diff),
        ],
        Some(&status),
        &[("edge", diff), ("large", &large_diff)],
    );

    assert_eq!(
        packet.changed_files.len(),
        WORKSPACE_REVIEW_MAX_CHANGED_FILES
    );
    assert!(packet.summary.files_changed > WORKSPACE_REVIEW_MAX_CHANGED_FILES as u32);
    assert!(packet.changed_files_truncated);
    assert_eq!(packet.summary.deletions, 2);
    assert!(packet.summary.insertions >= 4);
    assert!(packet.patch_excerpt_truncated);
    assert_eq!(
        packet.patch_excerpt.chars().count(),
        WORKSPACE_REVIEW_PATCH_EXCERPT_CHARS
    );
    assert!(packet
        .notes
        .iter()
        .any(|note| note.contains("Untracked files are listed")));
    assert!(packet
        .notes
        .iter()
        .any(|note| note.contains("Changed file list is limited")));
    assert!(packet
        .notes
        .iter()
        .any(|note| note.contains("Patch excerpt is limited")));
    assert!(!packet.patch_excerpt.contains("### empty diff"));

    let file = |path: &str| {
        packet
            .changed_files
            .iter()
            .find(|file| file.path == path)
            .expect("changed file should be listed")
    };
    assert_eq!(file("added.rs").status, "added");
    assert_eq!(file("deleted.rs").status, "deleted");
    assert_eq!(file("renamed file.rs").status, "renamed");
    assert_eq!(file("status_added.rs").status, "added");
    assert!(file("status_added.rs")
        .sources
        .contains(&"status".to_string()));
    assert!(!packet
        .changed_files
        .iter()
        .any(|file| file.path == "/dev/null" || file.path.is_empty()));

    let mut ranked_files = BTreeMap::<String, ChangedFileAccumulator>::new();
    add_changed_file(&mut ranked_files, "ranked.rs", "modified", "low");
    add_changed_file(&mut ranked_files, "ranked.rs", "unknown", "ignored");
    add_changed_file(&mut ranked_files, "ranked.rs", "untracked", "high");
    let ranked = ranked_files
        .get("ranked.rs")
        .expect("ranked file should be tracked");
    assert_eq!(ranked.status, "untracked");
    assert!(ranked.sources.contains("ignored"));
}

#[test]
fn review_packet_reports_typed_hunk_truncation_without_changing_small_packets() {
    let large_diff = (0..=WORKSPACE_REVIEW_MAX_HUNK_ANCHORS)
        .map(|index| {
            format!(
                "diff --git a/src/{index}.rs b/src/{index}.rs\n--- a/src/{index}.rs\n+++ b/src/{index}.rs\n@@ -1 +1 @@\n-old\n+new"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let truncated = build_selected_source_review_packet(&large_diff);
    assert!(truncated.hunk_anchors_truncated);
    assert_eq!(
        truncated.hunk_anchors.len(),
        WORKSPACE_REVIEW_MAX_HUNK_ANCHORS
    );

    let small = build_selected_source_review_packet(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new",
    );
    assert!(!small.changed_files_truncated);
    assert!(!small.hunk_anchors_truncated);
    assert!(!small.patch_excerpt_truncated);
}

#[test]
fn git_path_output_rejects_empty_and_resolves_git_paths() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let empty_error = git_path_output(temp.path(), " \n").expect_err("empty git path should fail");
    match empty_error {
        AppError::GitOperation(message) => assert!(message.contains("empty path")),
        other => panic!("expected GitOperation, got {other:?}"),
    }

    let relative =
        git_path_output(temp.path(), ".git/objects\n").expect("relative git path should resolve");
    assert_eq!(relative, temp.path().join(".git/objects"));

    let absolute_dir = temp.path().join("objects");
    let absolute = git_path_output(temp.path(), &format!("{}\n", absolute_dir.display()))
        .expect("absolute git path should pass through");
    assert_eq!(absolute, absolute_dir);
}

#[tokio::test]
async fn git_stdout_lossy_with_env_reports_git_failures() {
    let (_temp, repo, _base_sha) = init_repo();

    let error =
        git_stdout_lossy_with_env(&["rev-parse", "--verify", "refs/heads/missing"], &repo, &[])
            .await
            .expect_err("failed git command should return an error");

    match error {
        AppError::GitOperation(message) => assert!(!message.trim().is_empty()),
        other => panic!("expected GitOperation, got {other:?}"),
    }
}

#[tokio::test]
async fn workspace_delta_content_fingerprint_tracks_content_not_head_provenance() {
    let (_temp, repo, base_sha) = init_repo();
    std::fs::write(repo.join("README.md"), "base\nupdated\n")
        .expect("tracked file should be changed");
    std::fs::write(repo.join("untracked.rs"), "pub fn added() {}\n")
        .expect("untracked file should be written");

    let uncommitted_fingerprint = workspace_delta_content_fingerprint(&repo, &base_sha)
        .await
        .expect("uncommitted content should fingerprint");

    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "commit equivalent content"]);
    let committed_fingerprint = workspace_delta_content_fingerprint(&repo, &base_sha)
        .await
        .expect("committed content should fingerprint");

    assert_eq!(committed_fingerprint, uncommitted_fingerprint);

    std::fs::write(
        repo.join("untracked.rs"),
        "pub fn added() { println!(\"changed\"); }\n",
    )
    .expect("content should change");
    let changed_fingerprint = workspace_delta_content_fingerprint(&repo, &base_sha)
        .await
        .expect("changed content should fingerprint");

    assert_ne!(changed_fingerprint, uncommitted_fingerprint);
}

#[tokio::test]
async fn workspace_review_source_snapshot_fingerprint_uses_background_git_lane() {
    let (_temp, repo, base_sha) = init_repo();
    std::fs::write(repo.join("README.md"), "base\nupdated\n")
        .expect("tracked file should be changed");
    let target = AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        base_ref: base_sha,
        base_sha: None,
        head_ref: "HEAD".to_string(),
        head_sha: None,
        diff_fingerprint: "initial".to_string(),
        working_directory: repo,
        source_pull_request_number: None,
        review_packet: AgentWorkspaceReviewPacket::default(),
    };
    let captured = StdArc::new(StdMutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(WorkspaceReviewGitLaneLayer {
        captured: StdArc::clone(&captured),
    });
    let _guard = subscriber.set_default();

    workspace_review_source_snapshot_fingerprint(&target)
        .await
        .expect("source snapshot should fingerprint");

    let events = captured
        .lock()
        .expect("git lane capture lock should remain available");
    assert!(
        !events.is_empty(),
        "fingerprinting must execute git commands"
    );
    assert!(
        events.iter().all(|event| event.lane == "background"),
        "all source snapshot commands must use the background lane: {events:?}"
    );
    assert!(
        events.iter().any(|event| event.command == "add -A -- ."),
        "the lane assertion must cover the whole-worktree add: {events:?}"
    );
}

#[tokio::test]
async fn workspace_delta_fingerprint_rejects_conflicted_index_created_during_resolution() {
    let (_temp, repo, base_sha) = init_repo();
    git(&repo, &["checkout", "-b", "conflict-source"]);
    std::fs::write(repo.join("README.md"), "source\n").expect("source file should change");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "source change"]);
    git(&repo, &["checkout", "main"]);
    std::fs::write(repo.join("README.md"), "main\n").expect("main file should change");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "main change"]);

    let (reached_tx, reached_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let fingerprint_repo = repo.clone();
    let fingerprint_task = std::thread::spawn(move || {
        let subscriber =
            tracing_subscriber::registry().with(WorkspaceReviewGitWriteTreeGateLayer {
                completed_write_trees: AtomicUsize::new(0),
                reached: reached_tx,
                resume: StdMutex::new(resume_rx),
            });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("fingerprint test runtime should build");
        tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(workspace_delta_tree_fingerprints(
                &fingerprint_repo,
                &base_sha,
            ))
        })
    });

    reached_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("fingerprinting should pause before the final settle check");
    let merge = Command::new("git")
        .args(["merge", "conflict-source"])
        .current_dir(&repo)
        .output()
        .expect("conflicting merge should spawn");
    assert!(
        !merge.status.success(),
        "merge must create an unmerged index"
    );
    std::fs::remove_file(repo.join(".git/MERGE_HEAD"))
        .expect("test should isolate an unmerged index without operation metadata");
    assert!(
        !GitService::unfinished_operation_state(&repo)
            .expect("operation state should load")
            .is_unfinished(),
        "the trailing conflict-files read, not operation metadata, must reject this state"
    );
    assert!(
        !GitService::get_conflict_files(&repo)
            .await
            .expect("conflict files should load")
            .is_empty(),
        "test precondition requires an unmerged index"
    );
    resume_tx
        .send(())
        .expect("fingerprint test should resume the final settle check");

    let error = match fingerprint_task
        .join()
        .expect("fingerprint task should join")
    {
        Ok(_) => panic!("a conflicted index created mid-fingerprint must be rejected"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        AppError::WorkspaceReviewUnfinishedGitOperation
    ));
}

#[tokio::test]
async fn load_context_resolves_workspace_delta_and_monitor_fields() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);
    std::fs::write(repo.join("staged.rs"), "pub fn staged() {}\n")
        .expect("staged file should be written");
    git(&repo, &["add", "staged.rs"]);
    std::fs::write(repo.join("unstaged.rs"), "pub fn unstaged() {}\n")
        .expect("unstaged file should be written");

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha.clone()),
    );

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace delta context should load");
    let target = context
        .target
        .expect("workspace delta should be reviewable");

    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::WorkspaceDelta
    );
    assert_eq!(target.base_ref, base_sha);
    assert_eq!(target.head_ref, "HEAD");
    assert!(target.base_sha.is_some());
    assert!(target.head_sha.is_some());
    assert!(!target.diff_fingerprint.is_empty());
    assert_eq!(target.working_directory, repo);
    assert_eq!(target.review_packet.summary.files_changed, 3);
    assert_eq!(target.review_packet.summary.insertions, 2);
    assert_eq!(target.review_packet.summary.deletions, 0);
    assert!(target.review_packet.changed_files.iter().any(|file| {
        file.path == "committed.rs" && file.sources.contains(&"committed".to_string())
    }));
    assert!(target
        .review_packet
        .changed_files
        .iter()
        .any(|file| { file.path == "staged.rs" && file.sources.contains(&"staged".to_string()) }));
    assert!(target
        .review_packet
        .changed_files
        .iter()
        .any(|file| file.path == "unstaged.rs" && file.status == "untracked"));
    assert!(target
        .review_packet
        .patch_excerpt
        .contains("### committed diff"));
    assert!(target
        .review_packet
        .patch_excerpt
        .contains("### staged diff"));
    assert!(target
        .review_packet
        .patch_excerpt
        .contains("### git status --porcelain=v1 -uall"));
    assert!(!context.is_current);
    assert!(!context.is_outdated);
    assert!(context.should_show_tab);
    assert_eq!(
        context.monitor.current_target_scope,
        Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
    );
    assert_eq!(context.monitor.workspace_head_ref.as_deref(), Some("HEAD"));
    assert_eq!(
        context.monitor.workspace_base_ref.as_deref(),
        Some(base_sha.as_str())
    );
}

#[tokio::test]
async fn load_context_resolves_selected_branch_when_workspace_has_no_delta() {
    let (temp, repo, _base_sha) = init_repo();
    git(&repo, &["checkout", "-b", "feature/source"]);
    std::fs::write(repo.join("feature.rs"), "pub fn feature() {}\n")
        .expect("feature file should be written");
    git(&repo, &["add", "feature.rs"]);
    git(&repo, &["commit", "-m", "feature change"]);
    let feature_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let missing_worktree = temp.path().join("missing-worktree");
    let workspace = workspace(
        &project,
        &missing_worktree,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "feature/source",
        None,
    );

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("selected branch context should load");
    let target = context
        .target
        .expect("selected branch should be reviewable");

    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::SelectedSource
    );
    assert_eq!(target.base_ref, "main");
    assert_eq!(target.head_ref, "feature/source");
    assert_eq!(target.head_sha.as_deref(), Some(feature_head.as_str()));
    assert_eq!(target.source_pull_request_number, None);
    assert_eq!(target.review_packet.summary.files_changed, 1);
    assert_eq!(target.review_packet.summary.insertions, 1);
    assert!(target.review_packet.changed_files.iter().any(|file| {
        file.path == "feature.rs" && file.sources.contains(&"selected_source".to_string())
    }));
    assert!(target
        .review_packet
        .patch_excerpt
        .contains("### selected_source diff"));
    assert_eq!(
        context.monitor.selected_source_head_ref.as_deref(),
        Some("feature/source")
    );
    assert!(context.should_show_tab);
}

#[tokio::test]
async fn load_context_resolves_selected_pull_request_metadata() {
    let (temp, repo, _base_sha) = init_repo();
    git(&repo, &["checkout", "-b", "feature/pr-42"]);
    std::fs::write(repo.join("pr.rs"), "pub fn pr() {}\n").expect("pr file should be written");
    git(&repo, &["add", "pr.rs"]);
    git(&repo, &["commit", "-m", "pr change"]);
    let pr_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &temp.path().join("missing-worktree"),
        IdeationAnalysisBaseRefKind::PullRequest,
        "feature/pr-42",
        None,
    );
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 42,
        url: Some("https://github.example/pr/42".to_string()),
        title: Some("Review source".to_string()),
        head_ref_name: "feature/pr-42".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some(pr_head.clone()),
    });

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("selected PR context should load");
    let target = context.target.expect("selected PR should be reviewable");

    assert_eq!(target.base_ref, "main");
    assert_eq!(target.head_ref, "feature/pr-42");
    assert_eq!(target.head_sha.as_deref(), Some(pr_head.as_str()));
    assert_eq!(target.source_pull_request_number, Some(42));
    assert_eq!(
        context.monitor.selected_source_pull_request_number,
        Some(42)
    );
}

#[tokio::test]
async fn load_context_resolves_published_pr_preserved_ref_and_terminal_merge_base() {
    let (temp, repo, base_sha) = init_repo();
    git(&repo, &["checkout", "-b", "feature/published-pr"]);
    std::fs::write(repo.join("published.rs"), "pub fn published() {}\n")
        .expect("published file should be written");
    git(&repo, &["add", "published.rs"]);
    git(&repo, &["commit", "-m", "published pr change"]);
    let pr_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["update-ref", "refs/ralphx/pr-heads/483", &pr_head]);
    git(&repo, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &temp.path().join("missing-worktree"),
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        None,
    );
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some("merged".to_string());

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("published PR context should load");
    let target = context.target.expect("published PR should be reviewable");

    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::SelectedSource
    );
    assert_eq!(target.base_ref, base_sha);
    assert_eq!(target.head_ref, "refs/ralphx/pr-heads/483");
    assert_eq!(target.head_sha.as_deref(), Some(pr_head.as_str()));
    assert_eq!(target.source_pull_request_number, Some(483));
    assert_eq!(
        context.monitor.selected_source_base_ref.as_deref(),
        Some(target.base_ref.as_str())
    );
}

#[tokio::test]
async fn load_context_handles_missing_sources_without_review_tab() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let missing_repo = temp.path().join("missing-repo");
    let state = AppState::new_test();
    let project = seed_project(&state, &missing_repo).await;
    let workspace = workspace(
        &project,
        &temp.path().join("missing-worktree"),
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        None,
    );

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("empty context should load");

    assert!(context.target.is_none());
    assert_eq!(
        context.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Idle
    );
    assert!(!context.is_current);
    assert!(!context.is_outdated);
    assert!(!context.should_show_tab);
}

#[tokio::test]
async fn manual_blocking_review_fixer_requires_current_review_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let missing_repo = temp.path().join("missing-repo");
    let state = AppState::new_test();
    let project = seed_project(&state, &missing_repo).await;
    let workspace = workspace(
        &project,
        &temp.path().join("missing-worktree"),
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        None,
    );

    let error = start_agent_workspace_review_blocking_fixer(&state, &workspace)
        .await
        .expect_err("manual fixer should require a reviewable target");

    assert!(matches!(
        error,
        AppError::Validation(message) if message.contains("current review target")
    ));
}

#[tokio::test]
async fn existing_review_artifact_marks_context_current_then_outdated() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("initial context should load");
    let target = initial.target.expect("initial target should exist");
    let mut monitor = initial.monitor;
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-1".to_string()),
        ArtifactId::from_string("artifact-1"),
        1,
        Utc::now(),
        None,
        ArtifactId::from_string("artifact-requested-changes-1"),
        1,
        Utc::now(),
        None,
    );
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    let current = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("current context should load");
    assert!(current.is_current);
    assert!(!current.is_outdated);
    assert_eq!(
        current.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Ready
    );

    std::fs::write(repo.join("later.rs"), "pub fn later() {}\n")
        .expect("later file should be written");
    let outdated = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("outdated context should load");
    assert!(!outdated.is_current);
    assert!(outdated.is_outdated);
    assert!(outdated.should_show_tab);
}

#[tokio::test]
async fn overview_only_workspace_review_is_readable_but_cannot_authorize_currentness() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("initial context should load");
    let target = initial.target.expect("initial target should exist");
    let mut monitor = initial.monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.reviewed_target_scope = Some(target.scope);
    monitor.reviewed_head_sha = target.head_sha.clone();
    monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint);
    monitor.review_artifact_id = Some(ArtifactId::from_string("legacy-overview"));
    monitor.review_artifact_version = Some(2);
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("legacy monitor should persist");

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("legacy context should remain readable");
    assert!(!context.is_current);
    assert!(context.is_outdated);
    assert_eq!(
        context.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Required
    );
}

#[tokio::test]
async fn passing_workspace_review_survives_equivalent_commit_then_invalidates_on_content_change() {
    let (_temp, repo, base_sha) = init_repo();
    std::fs::write(repo.join("README.md"), "base\nupdated\n")
        .expect("tracked file should be changed");
    std::fs::write(repo.join("new_file.rs"), "pub fn new_file() {}\n")
        .expect("untracked file should be written");

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("initial context should load");
    let target = initial.target.expect("initial target should exist");
    let reviewed_head_sha = target.head_sha.clone();
    let mut monitor = initial.monitor;
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-equivalent".to_string()),
        ArtifactId::from_string("artifact-equivalent"),
        1,
        Utc::now(),
        None,
        ArtifactId::from_string("artifact-equivalent-requested-changes"),
        1,
        Utc::now(),
        None,
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    let before_commit = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("pre-commit context should load");
    assert!(before_commit.is_current);
    assert!(!before_commit.is_outdated);

    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "publish equivalent content"]);
    let committed_head_sha = git(&repo, &["rev-parse", "HEAD"]);

    let after_commit = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("post-commit context should load");
    let after_commit_target = after_commit
        .target
        .as_ref()
        .expect("post-commit target should exist");
    assert_ne!(
        reviewed_head_sha.as_deref(),
        Some(committed_head_sha.as_str())
    );
    assert_eq!(
        after_commit_target.head_sha.as_deref(),
        Some(committed_head_sha.as_str())
    );
    assert!(
        after_commit.is_current,
        "equivalent committed content should not invalidate the Review"
    );
    assert!(!after_commit.is_outdated);
    assert_eq!(
        after_commit.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );

    std::fs::write(
        repo.join("new_file.rs"),
        "pub fn new_file() { println!(\"changed\"); }\n",
    )
    .expect("reviewed file should change after commit");
    let changed = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("changed context should load");
    assert!(!changed.is_current);
    assert!(changed.is_outdated);
    assert_eq!(
        changed.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Required
    );
}

#[tokio::test]
async fn stale_approval_retry_does_not_refresh_or_clear_monitor_before_cas() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("initial context should load");
    let target = initial.target.expect("initial target should exist");
    let approved_at = Utc::now();
    let artifact_id = ArtifactId::from_string("artifact-approved-anyway".to_string());
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: target.scope,
        diff_fingerprint: target.diff_fingerprint.clone(),
        artifact_id: artifact_id.clone(),
        artifact_version: 7,
    };
    let mut monitor = initial.monitor;
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-approved-anyway".to_string()),
        artifact_id,
        snapshot.artifact_version,
        approved_at,
        None,
        ArtifactId::from_string("artifact-approved-anyway-requested-changes"),
        7,
        approved_at,
        None,
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_blocking_summary = Some("blockers remain".to_string());
    monitor.review_gate_bypassed_at = Some(approved_at);
    monitor.review_gate_bypassed_target_scope = Some(snapshot.target_scope);
    monitor.review_gate_bypassed_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
    monitor.review_gate_bypassed_artifact_id = Some(snapshot.artifact_id.clone());
    monitor.review_gate_bypassed_artifact_version = Some(snapshot.artifact_version);
    let persisted_before = state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    std::fs::write(repo.join("later.rs"), "pub fn later() {}\n")
        .expect("later file should be written");
    let error = approve_agent_workspace_review_anyway(&state, &workspace, &snapshot)
        .await
        .expect_err("stale approval retry should be rejected");

    assert!(matches!(error, AppError::Conflict(message) if message.contains("changed before")));
    let persisted_after = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should still exist");
    assert_eq!(
        persisted_after.current_diff_fingerprint,
        persisted_before.current_diff_fingerprint
    );
    assert_eq!(
        persisted_after.review_gate_status,
        persisted_before.review_gate_status
    );
    assert_eq!(
        persisted_after.review_gate_bypassed_at,
        persisted_before.review_gate_bypassed_at
    );
    assert_eq!(
        persisted_after.review_gate_bypassed_diff_fingerprint,
        persisted_before.review_gate_bypassed_diff_fingerprint
    );
    assert_eq!(
        persisted_after.review_gate_bypassed_artifact_id,
        persisted_before.review_gate_bypassed_artifact_id
    );
}

#[tokio::test]
async fn approval_rejects_active_publish_before_project_lookup_or_monitor_write() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let project = Project::new(
        "Workspace Review Missing Project".to_string(),
        repo.to_string_lossy().to_string(),
    );
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    workspace.publication_push_status = Some("pushing".to_string());
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-active-publish".to_string(),
        artifact_id: ArtifactId::from_string("artifact-active-publish"),
        artifact_version: 1,
    };

    let error = approve_agent_workspace_review_anyway(&state, &workspace, &snapshot)
        .await
        .expect_err("active publish must block human approval before any refresh");

    assert!(matches!(error, AppError::Conflict(message) if message.contains("Commit & Publish")));
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .is_none());
}

#[tokio::test]
async fn approval_rejects_missing_project_before_target_resolution() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let project = Project::new(
        "Workspace Review Missing Project".to_string(),
        repo.to_string_lossy().to_string(),
    );
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-missing-project".to_string(),
        artifact_id: ArtifactId::from_string("artifact-missing-project"),
        artifact_version: 1,
    };

    let error = approve_agent_workspace_review_anyway(&state, &workspace, &snapshot)
        .await
        .expect_err("approval should fail closed when the project row is missing");

    assert!(matches!(error, AppError::NotFound(message) if message.contains("Project not found")));
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .is_none());
}

#[tokio::test]
async fn approval_rejects_when_current_workspace_has_no_reviewable_target() {
    let (_temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-no-target".to_string(),
        artifact_id: ArtifactId::from_string("artifact-no-target"),
        artifact_version: 1,
    };

    let error = approve_agent_workspace_review_anyway(&state, &workspace, &snapshot)
        .await
        .expect_err("approval requires a current reviewable target");

    assert!(matches!(error, AppError::Conflict(message) if message.contains("no longer matches")));
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .is_none());
}

#[tokio::test]
async fn approval_rejects_reviewable_target_without_existing_monitor() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-without-monitor".to_string(),
        artifact_id: ArtifactId::from_string("artifact-without-monitor"),
        artifact_version: 1,
    };

    let error = approve_agent_workspace_review_anyway(&state, &workspace, &snapshot)
        .await
        .expect_err("approval requires an existing current blocking monitor");

    assert!(matches!(error, AppError::Conflict(message) if message.contains("changed before")));
    assert!(state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("event read should succeed")
        .is_empty());
}

#[tokio::test]
async fn start_review_skips_current_and_already_reviewing_targets() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = Arc::new(AppState::new_test());
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("initial context should load");
    let target = initial.target.expect("target should exist");

    let mut current_monitor = initial.monitor.clone();
    apply_review_artifact_to_monitor(
        &mut current_monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-current".to_string()),
        ArtifactId::from_string("artifact-current"),
        2,
        Utc::now(),
        None,
    );
    current_monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    current_monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(current_monitor)
        .await
        .expect("current monitor should persist");
    let current_start = start_agent_workspace_review(Arc::clone(&state), &workspace, false)
        .await
        .expect("current start should not spawn");
    assert!(!current_start.started);
    assert_eq!(current_start.skipped_reason.as_deref(), Some("current"));
    assert_eq!(
        current_start.context.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Ready
    );

    let mut reviewing_monitor =
        AgentWorkspaceReviewMonitor::new(workspace.conversation_id.clone(), project.id.clone());
    apply_current_target_to_monitor(&mut reviewing_monitor, Some(&target));
    reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor)
        .await
        .expect("reviewing monitor should persist");
    let reviewing_start = start_agent_workspace_review(state, &workspace, false)
        .await
        .expect("reviewing start should not spawn");
    assert!(!reviewing_start.started);
    assert_eq!(
        reviewing_start.skipped_reason.as_deref(),
        Some("already_reviewing")
    );
}

#[tokio::test]
async fn refreshed_review_target_invalidates_the_previous_fixer_attempt() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);
    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.review_blocking_summary = Some("Old blocker".to_string());
    monitor.review_blocking_fingerprint = Some("blocker-old".to_string());
    monitor.review_fixer_status = Some(WORKSPACE_REVIEW_FIXER_STATUS_ROUTING.to_string());
    monitor.review_fixer_attempt_id = Some("attempt-old".to_string());

    let mut refreshed_target = target;
    refreshed_target.diff_fingerprint = "diff-refreshed".to_string();
    apply_current_target_to_monitor(&mut monitor, Some(&refreshed_target));

    assert_eq!(
        monitor.current_diff_fingerprint.as_deref(),
        Some("diff-refreshed")
    );
    assert_eq!(monitor.review_blocking_summary, None);
    assert_eq!(monitor.review_blocking_fingerprint, None);
    assert_eq!(monitor.review_fixer_status, None);
    assert_eq!(monitor.review_fixer_attempt_id, None);
}

#[tokio::test]
async fn start_review_runs_workspace_reviewer_child_chat_and_records_blocked_completion() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let agent_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let mut plan_artifact = Artifact::new_inline(
        "Approved implementation plan",
        ArtifactType::Specification,
        "# Plan\n\nUse the backend-owned Review gate.",
        "ralphx-ideation",
    );
    plan_artifact.metadata.version = 4;
    let plan_artifact = state
        .artifact_repo
        .create(plan_artifact)
        .await
        .expect("plan artifact should persist");
    let blueprint_artifact = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Implementation blueprint",
            ArtifactType::Specification,
            "# Blueprint\n\nImplement the workspace review plan.",
            "ralphx-ideation",
        ))
        .await
        .expect("blueprint artifact should persist");
    let planning_session = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .plan_artifact_id(plan_artifact.id.clone())
        .plan_blueprint_artifact_id(blueprint_artifact.id.clone())
        .build();
    let planning_session = state
        .ideation_session_repo
        .create(planning_session)
        .await
        .expect("planning session should persist");
    workspace.linked_ideation_session_id = Some(planning_session.id.clone());
    seed_conversation(&state, &workspace).await;
    let mut parent_message = ChatMessage::user_in_project(project.id.clone(), "Build it");
    parent_message.conversation_id = Some(workspace.conversation_id.clone());
    parent_message.metadata = Some(
        serde_json::json!({
            "composer_project_references": [
                { "path": "README.md", "kind": "file" }
            ],
            "composer_integration_references": [
                {
                    "provider": "atlassian",
                    "kind": "jira",
                    "id": "RX-42",
                    "key": "RX-42",
                    "title": "Fix Review gate",
                    "url": "https://jira.test/browse/RX-42"
                },
                {
                    "provider": "clickup",
                    "kind": "clickup",
                    "id": "task-1",
                    "key": "CU-1",
                    "title": "ClickUp review task",
                    "url": "https://clickup.test/t/task-1"
                }
            ],
            "composer_artifact_references": [
                {
                    "artifactId": "stale-plan",
                    "kind": "plan",
                    "title": "Stale plan"
                },
                {
                    "artifactId": "design-artifact-1",
                    "kind": "design",
                    "title": "Design context"
                },
                { "artifactId": "design-artifact-2", "kind": "design" },
                { "artifactId": "design-artifact-3", "kind": "design" },
                { "artifactId": "design-artifact-4", "kind": "design" },
                { "artifactId": "design-artifact-5", "kind": "design" },
                { "artifactId": "design-artifact-6", "kind": "design" },
                { "artifactId": "design-artifact-7", "kind": "design" },
                { "artifactId": "design-artifact-8", "kind": "design" }
            ]
        })
        .to_string(),
    );
    state
        .chat_message_repo
        .create(parent_message)
        .await
        .expect("parent message should persist");
    let mut hidden_message =
        ChatMessage::user_in_project(project.id.clone(), "Hidden recovery details");
    hidden_message.conversation_id = Some(workspace.conversation_id.clone());
    hidden_message.metadata = Some(
        serde_json::json!({
            "hidden_from_ui": true,
            "composer_project_references": [
                { "path": "hidden-recovery.md", "kind": "file" }
            ],
            "composer_integration_references": [
                {
                    "provider": "linear",
                    "kind": "issue",
                    "id": "LIN-HIDDEN",
                    "title": "Hidden issue"
                }
            ],
            "composer_artifact_references": [
                {
                    "artifactId": "hidden-artifact",
                    "kind": "notes",
                    "title": "Hidden notes"
                }
            ]
        })
        .to_string(),
    );
    state
        .chat_message_repo
        .create(hidden_message)
        .await
        .expect("hidden parent message should persist");

    let (_timing_guard, captured_timings) = capture_workspace_review_timings();
    let start = start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        &chat_service,
    )
    .await
    .expect("review child chat should start");
    assert_workspace_review_timing_phases(
        &captured_timings,
        "workspace_review_start_phase",
        &[
            "load_workspace",
            "load_project",
            "resolve_target",
            "load_monitor",
            "load_inherited_references",
            "validate_parent_conversation",
            "load_latest_run",
            "resolve_runtime",
            "create_child_conversation",
            "reserve_monitor",
            "start_child_chat",
            "append_publication_event",
            "total",
        ],
    );

    assert!(start.started);
    assert_eq!(start.skipped_reason, None);
    assert_eq!(
        start.context.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing
    );
    assert_eq!(
        start.context.goal_context.user_request_excerpts,
        vec!["Build it".to_string()]
    );
    assert!(start
        .context
        .goal_context
        .artifact_references
        .iter()
        .any(
            |reference| reference.artifact_id == plan_artifact.id.as_str()
                && reference.kind == "plan"
        ));
    assert!(start
        .context
        .goal_context
        .resolved_artifacts
        .iter()
        .any(|artifact| artifact.artifact_id == plan_artifact.id.as_str()
            && artifact.kind == "plan"
            && artifact
                .content
                .contains("Use the backend-owned Review gate.")
            && !artifact.content_truncated));
    assert!(start.context.monitor.last_run_id.is_some());
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0]
            .preallocated_agent_run_id
            .map(|run_id| run_id.to_string()),
        start.context.monitor.last_run_id
    );
    assert_eq!(
        sent_options[0].queue_policy,
        crate::application::chat_service::SendQueuePolicy::RequireImmediateStart
    );
    let review_conversation_id = start
        .context
        .monitor
        .review_conversation_id
        .clone()
        .expect("review conversation id should be recorded");
    let review_conversation = state
        .chat_conversation_repo
        .get_by_id(&review_conversation_id)
        .await
        .expect("review conversation lookup should succeed")
        .expect("review conversation should exist");
    let parent_conversation_id = workspace.conversation_id.as_str();
    assert_eq!(
        review_conversation.parent_conversation_id.as_deref(),
        Some(parent_conversation_id.as_str())
    );
    assert_eq!(review_conversation.context_type, ChatContextType::Project);
    assert_eq!(review_conversation.context_id, project.id.as_str());
    assert_eq!(
        review_conversation.title.as_deref(),
        Some("Review workspace changes")
    );

    let sent_messages = chat_service.get_sent_messages().await;
    assert_eq!(sent_messages.len(), 1);
    let review_prompt = &sent_messages[0];
    assert!(review_prompt.contains("Create or refresh the Review"));
    assert!(review_prompt.contains("- Scope: workspace_delta"));
    assert!(review_prompt.contains("<workspace_goal_context>"));
    assert!(review_prompt.contains("Goal Wins"));
    assert!(review_prompt.contains("Build it"));
    assert!(review_prompt.contains(plan_artifact.id.as_str()));
    assert!(review_prompt.contains("<resolved_artifact"));
    assert!(review_prompt.contains("Use the backend-owned Review gate."));
    assert!(review_prompt.contains("RX-42"));
    assert!(!review_prompt
        .contains("Fetch any `kind=&quot;plan&quot;` artifact reference with `get_artifact`"));
    assert!(review_prompt.contains("Use the target scope, head SHA, and diff fingerprint returned"));
    assert!(review_prompt.contains(&workspace.conversation_id.as_str()));
    assert!(!review_prompt.contains("pass conversation_id"));

    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    let options = &sent_options[0];
    assert_eq!(
        options.conversation_id_override,
        Some(review_conversation_id.clone())
    );
    assert_eq!(
        options.agent_name_override.as_deref(),
        Some(agent_names::AGENT_WORKSPACE_REVIEWER)
    );
    assert_eq!(
        options.working_directory_override.as_deref(),
        Some(repo.as_path())
    );
    assert_eq!(options.composer_project_references.len(), 1);
    assert_eq!(options.composer_project_references[0].path, "README.md");
    assert!(!options
        .composer_project_references
        .iter()
        .any(|reference| reference.path == "hidden-recovery.md"));
    assert_eq!(options.composer_integration_references.len(), 3);
    assert!(options
        .composer_integration_references
        .iter()
        .any(|reference| reference.provider == "atlassian"
            && reference.kind == "jira"
            && reference.key.as_deref() == Some("RX-42")));
    assert!(options
        .composer_integration_references
        .iter()
        .any(|reference| reference.id == "LIN-HIDDEN"));
    assert!(!options
        .composer_artifact_references
        .iter()
        .any(|reference| reference.artifact_id == "hidden-artifact"));
    assert!(options
        .composer_integration_references
        .iter()
        .any(|reference| reference.provider == "clickup"
            && reference.kind == "clickup"
            && reference.id == "task-1"));
    assert_eq!(options.composer_artifact_references.len(), 8);
    assert_eq!(
        options.composer_artifact_references[0].artifact_id,
        plan_artifact.id.as_str()
    );
    assert_eq!(
        options.composer_artifact_references[1].artifact_id,
        blueprint_artifact.id.as_str()
    );
    assert!(!options
        .composer_artifact_references
        .iter()
        .any(
            |reference| matches!(reference.kind.as_str(), "plan" | "plan_blueprint")
                && reference.session_id.as_deref() != Some(planning_session.id.as_str())
        ));
    assert!(options
        .composer_artifact_references
        .iter()
        .any(
            |reference| reference.artifact_id == "design-artifact-1" && reference.kind == "design"
        ));
    assert!(options
        .composer_artifact_references
        .iter()
        .any(
            |reference| reference.artifact_id == plan_artifact.id.as_str()
                && reference.kind == "plan"
                && reference.session_id.as_deref() == Some(planning_session.id.as_str())
                && reference.title.as_deref() == Some("Approved implementation plan")
                && reference.version == Some(4)
        ));
    assert!(options
        .composer_artifact_references
        .iter()
        .any(
            |reference| reference.artifact_id == blueprint_artifact.id.as_str()
                && reference.kind == "plan_blueprint"
                && reference.session_id.as_deref() == Some(planning_session.id.as_str())
        ));
    assert!(options.force_new_provider_session);
    let metadata: serde_json::Value = serde_json::from_str(
        options
            .metadata
            .as_deref()
            .expect("review kickoff should carry hidden message metadata"),
    )
    .expect("review kickoff metadata should be valid json");
    assert_eq!(metadata["hidden_from_ui"], true);
    assert_eq!(metadata["source"], "workspace_review_request");
    assert_eq!(
        metadata["plan_context_fingerprint"].as_str(),
        start
            .context
            .monitor
            .reviewed_plan_context_fingerprint
            .as_deref()
    );

    let mut blocked_monitor = None;
    for _ in 0..100 {
        if let Some(monitor) = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&workspace.conversation_id)
            .await
            .expect("monitor read should succeed")
        {
            if monitor.status == AgentWorkspaceReviewMonitorStatus::Blocked {
                blocked_monitor = Some(monitor);
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let blocked_monitor = blocked_monitor.expect("watcher should mark missing Review blocked");
    assert_eq!(
        blocked_monitor.last_run_id,
        start.context.monitor.last_run_id
    );
    assert_eq!(
        blocked_monitor.review_conversation_id,
        Some(review_conversation_id)
    );
    assert_eq!(
        blocked_monitor.last_error.as_deref(),
        Some("Workspace reviewer run disappeared before completion")
    );
}

#[tokio::test]
async fn start_review_blocks_monitor_when_child_chat_send_fails() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let agent_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
    let chat_service = MockChatService::new();
    chat_service.set_available(false).await;
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let error = start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        &chat_service,
    )
    .await
    .expect_err("review child chat send should fail");

    assert!(error
        .to_string()
        .contains("failed to start workspace reviewer chat"));
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    let review_conversation_id = sent_options[0]
        .conversation_id_override
        .clone()
        .expect("review conversation override should be created before send");
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should persist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(monitor.review_conversation_id, Some(review_conversation_id));
    assert_eq!(
        monitor.last_run_id,
        sent_options[0]
            .preallocated_agent_run_id
            .map(|run_id| run_id.to_string())
    );
    assert_eq!(
            monitor.last_error.as_deref(),
            Some(
                "failed to start workspace reviewer chat: Agent not available: Mock agent not available"
            )
        );
}

#[test]
fn full_packet_rejects_workspace_mutation_between_snapshots() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);
    let project = Project::new(
        "Workspace Review packet stability".to_string(),
        repo.to_string_lossy().to_string(),
    );
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let (reached_tx, reached_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let resolve_task = std::thread::spawn(move || {
        let subscriber =
            tracing_subscriber::registry().with(WorkspaceReviewGitWriteTreeGateLayer {
                completed_write_trees: AtomicUsize::new(0),
                reached: reached_tx,
                resume: StdMutex::new(resume_rx),
            });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("review target test runtime should build");
        tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(resolve_workspace_delta_target(
                &workspace,
                AgentWorkspaceReviewTargetMaterialization::FullPacket,
            ))
        })
    });

    reached_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("FullPacket resolution should pause after its initial snapshot");
    std::fs::write(
        repo.join("mutated-during-packet.rs"),
        "pub fn changed() {}\n",
    )
    .expect("workspace should mutate while packet reads are gated");
    resume_tx
        .send(())
        .expect("FullPacket resolution should resume after mutation");

    let error = resolve_task
        .join()
        .expect("FullPacket resolution task should join")
        .expect_err("unstable FullPacket capture must fail closed");
    assert!(matches!(error, AppError::Conflict(_)));
}

#[tokio::test]
async fn confirmed_preview_materializes_full_packet_before_start() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let agent_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    let mut target = resolve_review_target(&workspace, &project)
        .await
        .expect("target resolution should succeed")
        .expect("workspace delta target should exist");
    target.review_packet = AgentWorkspaceReviewPacket::default();

    let start = start_agent_workspace_review_with_revalidated_target_and_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        Some(target),
        &chat_service,
    )
    .await
    .expect("confirmed identity should rematerialize a full packet");

    assert!(start.started);
    let sent_messages = chat_service.get_sent_messages().await;
    assert_eq!(sent_messages.len(), 1);
    assert!(sent_messages[0].contains("Review packet: 1 files changed"));
}

#[tokio::test]
async fn confirmed_preview_rejects_a_changed_target_before_start() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let agent_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    let mut confirmed_target = resolve_review_target(&workspace, &project)
        .await
        .expect("preview target resolution should succeed")
        .expect("preview target should exist");
    confirmed_target.review_packet = AgentWorkspaceReviewPacket::default();
    commit_followup_change(&repo);

    let error = start_agent_workspace_review_with_revalidated_target_and_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        Some(confirmed_target),
        &chat_service,
    )
    .await
    .expect_err("changed target must invalidate the confirmation");

    assert!(matches!(error, AppError::Conflict(_)));
    assert!(chat_service.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn start_review_blocks_monitor_when_child_chat_breaks_reserved_identity() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let agent_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = Arc::new(AppState::new_test().with_agent_client(agent_client));
    let chat_service = MockChatService::new();
    chat_service.mismatch_next_send_result_identity().await;
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let error = start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        &chat_service,
    )
    .await
    .expect_err("review child chat must preserve its reserved identity");

    assert!(error
        .to_string()
        .contains("reserved immediate-start authority"));
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("reserved monitor should persist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.last_run_id,
        sent_options[0]
            .preallocated_agent_run_id
            .map(|run_id| run_id.to_string())
    );
    assert!(monitor
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("reserved immediate-start authority")));
}

#[tokio::test]
async fn start_review_blocks_monitor_without_sending_when_no_enabled_default_exists() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let mut state = AppState::new_test();
    state.agent_provider_settings_repo =
        Arc::new(crate::infrastructure::memory::MemoryAgentProviderSettingsRepository::new());
    let state = Arc::new(state);
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let error = start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        &chat_service,
    )
    .await
    .expect_err("missing enabled default provider should block review start");

    assert!(error.to_string().contains("Settings > Harness > Providers"));
    assert!(chat_service.get_sent_options().await.is_empty());
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should persist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert!(monitor.last_run_id.is_none());
    assert!(monitor.review_conversation_id.is_none());
    assert!(monitor
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("Settings > Harness > Providers")));
}

#[tokio::test]
async fn start_review_rejects_missing_parent_conversation_before_creating_a_child() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = Arc::new(AppState::new_test());
    let chat_service = MockChatService::new();
    chat_service.set_available(false).await;
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );

    let error = start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        &chat_service,
    )
    .await
    .expect_err("a missing parent conversation must fail before child launch");

    assert!(error.to_string().contains("Conversation not found"));
    assert!(chat_service.get_sent_options().await.is_empty());
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed");
    assert!(monitor.is_none());
}

#[tokio::test]
async fn start_review_uses_the_workspace_reviewer_role_default() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = Arc::new(
        AppState::new_test()
            .with_agent_client(default_client)
            .with_harness_agent_client(AgentHarnessKind::Codex, codex_client),
    );
    let chat_service = MockChatService::new();
    chat_service.set_available(false).await;
    let project = seed_project(&state, &repo).await;
    state
        .manual_role_default_repo
        .upsert_for_project(
            project.id.as_str(),
            RoutingRole::WorkspaceReviewer,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-project-review".to_string()),
                effort: Some(LogicalEffort::High),
                service_tier: ManualServiceTier::Standard,
                coordination_mode: None,
                persona_id: None,
                approval_policy: Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string()),
                sandbox_mode: Some(CODEX_DEFAULT_SANDBOX_MODE.to_string()),
                atlassian_access: None,
            },
        )
        .await
        .unwrap();
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let mut conversation =
        ChatConversation::new_task(TaskId::from_string("workspace-owner-task".to_string()));
    conversation.id = workspace.conversation_id.clone();
    conversation.agent_mode = Some(workspace.mode);
    conversation.provider_harness = Some(AgentHarnessKind::Codex);
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("non-project owner conversation should persist");

    start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        &chat_service,
    )
    .await
    .expect_err("review child chat send should fail after options are recorded");

    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0].harness_override,
        Some(AgentHarnessKind::Codex)
    );
    assert_eq!(
        sent_options[0].model_override.as_deref(),
        Some("gpt-project-review")
    );
    assert_eq!(
        sent_options[0].logical_effort_override,
        Some(LogicalEffort::High)
    );
    assert_eq!(
        sent_options[0].runtime_source_override,
        Some(RuntimeSource::RoleDefault)
    );
}

#[tokio::test]
async fn start_review_prefers_an_explicit_runtime_override_over_the_reviewer_default() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test()
        .with_agent_client(default_client)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_client);
    let provider_repo =
        Arc::new(crate::infrastructure::memory::MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    provider_repo.upsert(&codex).await.unwrap();
    provider_repo
        .upsert(&AgentProviderSettings::disabled_defaults(
            AgentHarnessKind::Claude,
        ))
        .await
        .unwrap();
    state.agent_provider_settings_repo = provider_repo;
    let state = Arc::new(state);
    let chat_service = MockChatService::new();
    chat_service.set_available(false).await;
    let project = seed_project(&state, &repo).await;
    state
        .manual_role_default_repo
        .upsert_for_project(
            project.id.as_str(),
            RoutingRole::WorkspaceReviewer,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-settings-medium".to_string()),
                effort: Some(LogicalEffort::Medium),
                service_tier: ManualServiceTier::Fast,
                coordination_mode: None,
                persona_id: None,
                approval_policy: Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string()),
                sandbox_mode: Some(CODEX_DEFAULT_SANDBOX_MODE.to_string()),
                atlassian_access: None,
            },
        )
        .await
        .unwrap();
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = workspace.conversation_id.clone();
    conversation.agent_mode = Some(workspace.mode);
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("owner conversation should persist");
    let runtime_override = ManualRoleRuntimeOverride {
        harness: AgentHarnessKind::Codex,
        model: Some("gpt-confirmed-high".to_string()),
        effort: Some(LogicalEffort::High),
        service_tier: ManualServiceTier::Standard,
        coordination_mode: None,
        persona_id: None,
    };

    start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        Some(&runtime_override),
        &chat_service,
    )
    .await
    .expect_err("review child chat send should fail after options are recorded");

    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0].harness_override,
        Some(AgentHarnessKind::Codex)
    );
    assert_eq!(
        sent_options[0].model_override.as_deref(),
        Some("gpt-confirmed-high")
    );
    assert_eq!(
        sent_options[0].logical_effort_override,
        Some(LogicalEffort::High)
    );
    assert_eq!(
        sent_options[0].service_tier_override.as_deref(),
        Some("standard")
    );
    assert_eq!(
        sent_options[0].runtime_source_override,
        Some(RuntimeSource::ConversationOverride)
    );
}

#[tokio::test]
async fn start_review_passes_the_provider_default_reviewer_model_to_the_child_chat() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test()
        .with_agent_client(default_client)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_client);
    let provider_repo =
        Arc::new(crate::infrastructure::memory::MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    codex.model = Some("gpt-5.6-terra".to_string());
    codex.effort = Some(LogicalEffort::XHigh);
    provider_repo.upsert(&codex).await.unwrap();
    state.agent_provider_settings_repo = provider_repo;
    let state = Arc::new(state);
    let chat_service = MockChatService::new();
    chat_service.set_available(false).await;
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = workspace.conversation_id.clone();
    conversation.agent_mode = Some(workspace.mode);
    conversation.provider_harness = Some(AgentHarnessKind::Claude);
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("owner conversation should persist");

    start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        true,
        None,
        &chat_service,
    )
    .await
    .expect_err("review child chat send should fail after options are recorded");

    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0].harness_override,
        Some(AgentHarnessKind::Codex)
    );
    assert_eq!(
        sent_options[0].model_override.as_deref(),
        Some("gpt-5.6-terra")
    );
    assert_eq!(
        sent_options[0].logical_effort_override,
        Some(LogicalEffort::XHigh)
    );
}

#[tokio::test]
async fn workspace_review_waiter_handles_failed_and_completed_child_runs() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = Arc::new(AppState::new_test());
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let mut failed_run = AgentRun::new(ChatConversationId::new());
    let failed_run_id = failed_run.id.as_str().to_string();
    failed_run.fail("review process crashed");
    state
        .agent_run_repo
        .create(failed_run)
        .await
        .expect("failed run should persist");
    let mut reviewing_monitor = context.monitor.clone();
    apply_current_target_to_monitor(&mut reviewing_monitor, Some(&target));
    reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    reviewing_monitor.last_run_id = Some(failed_run_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor)
        .await
        .expect("reviewing monitor should persist");

    spawn_workspace_review_waiter(
        Arc::clone(&state),
        workspace.clone(),
        target.clone(),
        failed_run_id.clone(),
        test_waiter_deadlines(),
    );

    let blocked = wait_for_monitor_status(
        &state,
        &workspace,
        AgentWorkspaceReviewMonitorStatus::Blocked,
    )
    .await;
    assert_eq!(blocked.last_run_id.as_deref(), Some(failed_run_id.as_str()));
    assert_eq!(
        blocked.last_error.as_deref(),
        Some("review process crashed")
    );

    let mut late_failed_run = AgentRun::new(ChatConversationId::new());
    let late_failed_run_id = late_failed_run.id.as_str().to_string();
    late_failed_run.fail("provider response generation failed after Review was saved");
    state
        .agent_run_repo
        .create(late_failed_run)
        .await
        .expect("late failed run should persist");
    let mut typed_ready_monitor = blocked.clone();
    apply_review_artifact_to_monitor(
        &mut typed_ready_monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(late_failed_run_id.clone()),
        ArtifactId::from_string("artifact-before-provider-failure"),
        3,
        Utc::now(),
        None,
    );
    typed_ready_monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    typed_ready_monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    typed_ready_monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(typed_ready_monitor)
        .await
        .expect("typed Review completion should persist");

    spawn_workspace_review_waiter(
        Arc::clone(&state),
        workspace.clone(),
        target.clone(),
        late_failed_run_id.clone(),
        test_waiter_deadlines(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let preserved = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(preserved.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(
        preserved.review_outcome,
        AgentWorkspaceReviewOutcome::Passed
    );
    assert_eq!(
        preserved.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
    assert_eq!(preserved.review_artifact_version, Some(3));
    assert_eq!(preserved.last_error, None);

    let mut completed_run = AgentRun::new(ChatConversationId::new());
    let completed_run_id = completed_run.id.as_str().to_string();
    completed_run.complete();
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("completed run should persist");
    let mut ready_monitor = preserved;
    apply_review_artifact_to_monitor(
        &mut ready_monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(completed_run_id.clone()),
        ArtifactId::from_string("artifact-ready"),
        4,
        Utc::now(),
        None,
    );
    ready_monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    ready_monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(ready_monitor)
        .await
        .expect("ready monitor should persist");

    spawn_workspace_review_waiter(
        Arc::clone(&state),
        workspace.clone(),
        target.clone(),
        completed_run_id.clone(),
        test_waiter_deadlines(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(
        monitor.last_run_id.as_deref(),
        Some(completed_run_id.as_str())
    );
    assert_eq!(monitor.review_artifact_version, Some(4));
    assert_eq!(monitor.last_error, None);

    let mut run_failed_completion = AgentRun::new(ChatConversationId::new());
    let run_failed_completion_id = run_failed_completion.id.as_str().to_string();
    run_failed_completion.complete();
    state
        .agent_run_repo
        .create(run_failed_completion)
        .await
        .expect("run_failed completion run should persist");
    let specific_error = "Workspace review packet requires additional hunk annotations".to_string();
    let mut run_failed_monitor = monitor;
    apply_current_target_to_monitor(&mut run_failed_monitor, Some(&target));
    apply_review_artifact_to_monitor(
        &mut run_failed_monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(run_failed_completion_id.clone()),
        ArtifactId::from_string("artifact-run-failed"),
        5,
        Utc::now(),
        None,
    );
    run_failed_monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
    run_failed_monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
    run_failed_monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
    run_failed_monitor.last_run_id = Some(run_failed_completion_id.clone());
    run_failed_monitor.last_error = Some(specific_error.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(run_failed_monitor)
        .await
        .expect("run_failed monitor should persist");

    spawn_workspace_review_waiter(
        Arc::clone(&state),
        workspace.clone(),
        target,
        run_failed_completion_id.clone(),
        test_waiter_deadlines(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(
        monitor.last_run_id.as_deref(),
        Some(run_failed_completion_id.as_str())
    );
    assert_eq!(monitor.review_artifact_version, Some(5));
    assert_eq!(monitor.last_error.as_deref(), Some(specific_error.as_str()));
}

// ── Liveness-aware waiter deadlines ──────────────────────────────────────────

/// Everything a deadline test needs: a persisted workspace, its current target, and a `Running`
/// reviewer child run whose `started_at` can be backdated to control the idle signal.
struct WaiterDeadlineFixture {
    _temp: tempfile::TempDir,
    state: Arc<AppState>,
    workspace: AgentConversationWorkspace,
    target: AgentWorkspaceReviewTarget,
    child_conversation_id: ChatConversationId,
    run_id: String,
}

/// Timeline repo whose activity read always fails, for proving fail-closed liveness handling.
/// Every other method delegates so the rest of the waiter behaves normally.
struct FailingChatTimelineRepository {
    inner: crate::infrastructure::memory::MemoryChatTimelineRepository,
}

#[async_trait::async_trait]
impl crate::domain::repositories::ChatTimelineRepository for FailingChatTimelineRepository {
    async fn upsert_item(
        &self,
        item: ChatTimelineItem,
    ) -> crate::error::AppResult<ChatTimelineItem> {
        self.inner.upsert_item(item).await
    }

    async fn get_by_id(
        &self,
        id: &crate::domain::entities::ChatTimelineItemId,
    ) -> crate::error::AppResult<Option<ChatTimelineItem>> {
        self.inner.get_by_id(id).await
    }

    async fn get_page(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        before_sequence: Option<i64>,
    ) -> crate::error::AppResult<crate::domain::entities::ChatTimelinePage> {
        self.inner
            .get_page(conversation_id, limit, before_sequence)
            .await
    }

    async fn count_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<u32> {
        self.inner.count_by_conversation(conversation_id).await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<Vec<ChatTimelineItem>> {
        self.inner.get_by_conversation(conversation_id).await
    }

    async fn latest_assistant_activity_at_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
        _assistant_role: MessageRole,
    ) -> crate::error::AppResult<Option<DateTime<Utc>>> {
        Err(crate::error::AppError::Infrastructure(
            "timeline activity read unavailable".to_string(),
        ))
    }

    async fn delete_message_items_except_block_indices(
        &self,
        message_id: &ChatMessageId,
        retained_block_indices: Vec<i64>,
    ) -> crate::error::AppResult<()> {
        self.inner
            .delete_message_items_except_block_indices(message_id, retained_block_indices)
            .await
    }

    async fn mark_message_items_finalized(
        &self,
        message_id: &ChatMessageId,
    ) -> crate::error::AppResult<()> {
        self.inner.mark_message_items_finalized(message_id).await
    }
}

async fn waiter_deadline_fixture(silent_for_secs: i64) -> WaiterDeadlineFixture {
    waiter_deadline_fixture_with(silent_for_secs, false).await
}

async fn waiter_deadline_fixture_with(
    silent_for_secs: i64,
    failing_activity_reads: bool,
) -> WaiterDeadlineFixture {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let mut state = AppState::new_test();
    if failing_activity_reads {
        state.chat_timeline_repo = Arc::new(FailingChatTimelineRepository {
            inner: crate::infrastructure::memory::MemoryChatTimelineRepository::new(),
        });
    }
    let state = Arc::new(state);
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(child_conversation_id.clone());
    run.started_at = Utc::now() - chrono::Duration::seconds(silent_for_secs);
    let run_id = run.id.as_str().to_string();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("running reviewer run should persist");

    let mut reviewing_monitor = context.monitor.clone();
    apply_current_target_to_monitor(&mut reviewing_monitor, Some(&target));
    reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    reviewing_monitor.review_conversation_id = Some(child_conversation_id.clone());
    reviewing_monitor.last_run_id = Some(run_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor)
        .await
        .expect("reviewing monitor should persist");

    WaiterDeadlineFixture {
        _temp,
        state,
        workspace,
        target,
        child_conversation_id,
        run_id,
    }
}

/// Persist one assistant timeline block for the reviewer child, stamped `activity_at`.
/// This is the signal that advances mid-turn; `chat_messages.created_at` does not.
async fn seed_reviewer_timeline_activity(
    state: &AppState,
    conversation_id: &ChatConversationId,
    block_index: i64,
    activity_at: DateTime<Utc>,
) {
    let mut item = ChatTimelineItem::for_message_block(
        ChatMessageId::new(),
        conversation_id.clone(),
        block_index,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::Text,
    );
    item.created_at = activity_at;
    item.updated_at = activity_at;
    state
        .chat_timeline_repo
        .upsert_item(item)
        .await
        .expect("reviewer timeline activity should persist");
}

/// Persist one assistant `chat_messages` row for the reviewer child, stamped `created_at`.
async fn seed_reviewer_message(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    conversation_id: &ChatConversationId,
    created_at: DateTime<Utc>,
) {
    let mut message =
        ChatMessage::user_in_project(workspace.project_id.clone(), "reviewer progress");
    message.conversation_id = Some(conversation_id.clone());
    message.role = MessageRole::Orchestrator;
    message.created_at = created_at;
    state
        .chat_message_repo
        .create(message)
        .await
        .expect("reviewer message should persist");
}

/// Mark the fixture's monitor as holding a current, complete Review artifact pair for `target`
/// without a typed outcome — the exact race window the completion grace exists for.
async fn persist_current_review_artifact_pair(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
    run_id: &str,
) -> AgentWorkspaceReviewMonitor {
    let mut monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(run_id.to_string()),
        ArtifactId::from_string("artifact-awaiting-completion"),
        9,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("current artifact pair should persist");
    monitor
}

async fn read_monitor(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AgentWorkspaceReviewMonitor {
    state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist")
}

/// Proof Obligations 1 and 9: a reviewer whose only `chat_messages` row is ancient but whose
/// timeline blocks keep updating is still producing, so the idle timeout must not fire — even
/// long past the old fixed 900s deadline.
#[tokio::test]
async fn workspace_review_waiter_defers_idle_timeout_while_reviewer_is_producing() {
    let fixture = waiter_deadline_fixture(3_600).await;
    // The reviewer's only `chat_messages` row is ancient — a single long turn never re-stamps it.
    seed_reviewer_message(
        &fixture.state,
        &fixture.workspace,
        &fixture.child_conversation_id,
        Utc::now() - chrono::Duration::seconds(3_000),
    )
    .await;

    // ...but its timeline blocks keep landing, which is what "still producing" actually looks like.
    let heartbeat_state = Arc::clone(&fixture.state);
    let heartbeat_conversation = fixture.child_conversation_id.clone();
    let heartbeat = tokio::spawn(async move {
        for block_index in 0..40 {
            seed_reviewer_timeline_activity(
                &heartbeat_state,
                &heartbeat_conversation,
                block_index,
                Utc::now(),
            )
            .await;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    });

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&fixture.state),
        fixture.workspace.clone(),
        fixture.target.clone(),
        fixture.run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_millis(200),
            max_wall_clock: Duration::from_secs(60),
            completion_grace: Duration::from_millis(50),
        },
        Arc::clone(&chat_service),
    );

    tokio::time::sleep(Duration::from_millis(600)).await;
    let monitor = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(
        monitor.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing,
        "a reviewer that keeps persisting timeline blocks must never trip the idle timeout, \
         even though its chat_messages row stayed frozen"
    );
    assert_eq!(monitor.last_error, None);
    assert!(chat_service.get_stop_agent_calls().await.is_empty());
    handle.abort();
    heartbeat.abort();
}

/// Proof Obligation 4: a genuinely silent reviewer fails with the "no Review" error.
#[tokio::test]
async fn workspace_review_waiter_times_out_without_review_when_reviewer_is_silent() {
    let fixture = waiter_deadline_fixture(3_600).await;

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&fixture.state),
        fixture.workspace.clone(),
        fixture.target.clone(),
        fixture.run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_millis(100),
            max_wall_clock: Duration::from_secs(60),
            completion_grace: Duration::from_millis(50),
        },
        Arc::clone(&chat_service),
    );
    handle.await.expect("waiter should settle");

    let monitor = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_ERR_TIMED_OUT_NO_REVIEW)
    );
}

/// Proof Obligation 5: the absolute cap ends even a continuously producing run.
#[tokio::test]
async fn workspace_review_waiter_wall_clock_cap_fires_for_continuously_active_run() {
    let fixture = waiter_deadline_fixture(0).await;
    seed_reviewer_timeline_activity(
        &fixture.state,
        &fixture.child_conversation_id,
        0,
        Utc::now(),
    )
    .await;

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&fixture.state),
        fixture.workspace.clone(),
        fixture.target.clone(),
        fixture.run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_secs(60),
            max_wall_clock: Duration::from_millis(150),
            completion_grace: Duration::from_millis(50),
        },
        Arc::clone(&chat_service),
    );
    handle.await.expect("waiter should settle");

    let monitor = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_ERR_TIMED_OUT_NO_REVIEW)
    );
}

/// Proof Obligations 3 and 11: grace expires with a current Review still unconfirmed, so the gate
/// fails with the *accurate* error and the orphaned child is stopped afterwards. The recorded
/// error must not be the stop-path text, which proves `stop_agent` ran after the block.
#[tokio::test]
async fn workspace_review_waiter_fails_unconfirmed_review_after_grace_and_stops_child() {
    let fixture = waiter_deadline_fixture(3_600).await;
    persist_current_review_artifact_pair(
        &fixture.state,
        &fixture.workspace,
        &fixture.target,
        &fixture.run_id,
    )
    .await;

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&fixture.state),
        fixture.workspace.clone(),
        fixture.target.clone(),
        fixture.run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_millis(100),
            max_wall_clock: Duration::from_secs(60),
            completion_grace: Duration::from_millis(300),
        },
        Arc::clone(&chat_service),
    );
    handle.await.expect("waiter should settle");

    let monitor = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_ERR_UNCONFIRMED_REVIEW)
    );
    assert_eq!(
        chat_service.get_stop_agent_calls().await,
        vec![(
            ChatContextType::Project,
            fixture.child_conversation_id.as_str()
        )]
    );
}

/// Proof Obligation 2: the typed completion that lands inside the grace window wins, and the
/// normal Passed outcome and gate survive the deadline.
#[tokio::test]
async fn workspace_review_waiter_preserves_typed_completion_that_lands_inside_grace() {
    let fixture = waiter_deadline_fixture(3_600).await;
    persist_current_review_artifact_pair(
        &fixture.state,
        &fixture.workspace,
        &fixture.target,
        &fixture.run_id,
    )
    .await;

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&fixture.state),
        fixture.workspace.clone(),
        fixture.target.clone(),
        fixture.run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_millis(100),
            max_wall_clock: Duration::from_secs(60),
            completion_grace: Duration::from_secs(5),
        },
        Arc::clone(&chat_service),
    );

    // Land the reviewer's typed completion after the deadline has already tripped.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let mut completed = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(
        completed.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing,
        "grace must keep the monitor reviewable instead of failing it immediately"
    );
    completed.status = AgentWorkspaceReviewMonitorStatus::Ready;
    completed.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    completed.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    fixture
        .state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(completed)
        .await
        .expect("typed completion should persist");

    handle.await.expect("waiter should settle");

    let monitor = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(monitor.review_outcome, AgentWorkspaceReviewOutcome::Passed);
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
    assert_eq!(monitor.last_error, None);
    assert!(chat_service.get_stop_agent_calls().await.is_empty());
}

/// Proof Obligation 6: a typed completion already durable when the deadline trips is preserved,
/// reusing the same guard the run-terminal path uses.
#[tokio::test]
async fn workspace_review_waiter_preserves_typed_completion_present_at_deadline() {
    let fixture = waiter_deadline_fixture(3_600).await;
    let mut monitor = persist_current_review_artifact_pair(
        &fixture.state,
        &fixture.workspace,
        &fixture.target,
        &fixture.run_id,
    )
    .await;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    fixture
        .state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("typed completion should persist");

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&fixture.state),
        fixture.workspace.clone(),
        fixture.target.clone(),
        fixture.run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_millis(100),
            max_wall_clock: Duration::from_secs(60),
            completion_grace: Duration::from_millis(50),
        },
        Arc::clone(&chat_service),
    );
    handle.await.expect("waiter should settle");

    let monitor = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::Blocking
    );
    assert_eq!(monitor.last_error, None);
    assert!(chat_service.get_stop_agent_calls().await.is_empty());
}

/// Proof Obligation 10: a failing liveness read must never look like idleness. Only the absolute
/// wall-clock cap may still fire when activity cannot be read.
#[tokio::test]
async fn workspace_review_waiter_treats_failed_liveness_read_as_active() {
    let fixture = waiter_deadline_fixture_with(3_600, true).await;

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&fixture.state),
        fixture.workspace.clone(),
        fixture.target.clone(),
        fixture.run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_millis(100),
            max_wall_clock: Duration::from_secs(60),
            completion_grace: Duration::from_millis(50),
        },
        Arc::clone(&chat_service),
    );

    tokio::time::sleep(Duration::from_millis(400)).await;
    let monitor = read_monitor(&fixture.state, &fixture.workspace).await;
    assert_eq!(
        monitor.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing,
        "an unreadable activity signal must not be treated as idleness"
    );
    assert!(chat_service.get_stop_agent_calls().await.is_empty());
    handle.abort();
}

/// Wraps the memory agent-run repo and fails `get_by_id` while `should_fail` is set,
/// letting setup writes through before the flag is raised.
struct FailingAgentRunRepository {
    inner: Arc<crate::infrastructure::memory::MemoryAgentRunRepository>,
    should_fail: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl crate::domain::repositories::AgentRunRepository for FailingAgentRunRepository {
    async fn create(
        &self,
        run: crate::domain::entities::AgentRun,
    ) -> crate::error::AppResult<crate::domain::entities::AgentRun> {
        self.inner.create(run).await
    }

    async fn get_by_id(
        &self,
        id: &crate::domain::entities::AgentRunId,
    ) -> crate::error::AppResult<Option<crate::domain::entities::AgentRun>> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            Err(crate::error::AppError::Infrastructure(
                "simulated run-repo read failure".to_string(),
            ))
        } else {
            self.inner.get_by_id(id).await
        }
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &crate::domain::entities::ChatConversationId,
    ) -> crate::error::AppResult<Option<crate::domain::entities::AgentRun>> {
        self.inner.get_latest_for_conversation(conversation_id).await
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &crate::domain::entities::ChatConversationId,
    ) -> crate::error::AppResult<Option<crate::domain::entities::AgentRun>> {
        self.inner.get_active_for_conversation(conversation_id).await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &crate::domain::entities::ChatConversationId,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::AgentRun>> {
        self.inner.get_by_conversation(conversation_id).await
    }

    async fn update_status(
        &self,
        id: &crate::domain::entities::AgentRunId,
        status: crate::domain::entities::AgentRunStatus,
    ) -> crate::error::AppResult<()> {
        self.inner.update_status(id, status).await
    }

    async fn update_usage(
        &self,
        id: &crate::domain::entities::AgentRunId,
        usage: &crate::domain::entities::AgentRunUsage,
    ) -> crate::error::AppResult<()> {
        self.inner.update_usage(id, usage).await
    }

    async fn update_attribution(
        &self,
        id: &crate::domain::entities::AgentRunId,
        attribution: &crate::domain::entities::AgentRunAttribution,
    ) -> crate::error::AppResult<()> {
        self.inner.update_attribution(id, attribution).await
    }

    async fn complete(
        &self,
        id: &crate::domain::entities::AgentRunId,
    ) -> crate::error::AppResult<()> {
        self.inner.complete(id).await
    }

    async fn complete_if_prune_cancelled(
        &self,
        id: &crate::domain::entities::AgentRunId,
    ) -> crate::error::AppResult<bool> {
        self.inner.complete_if_prune_cancelled(id).await
    }

    async fn fail(
        &self,
        id: &crate::domain::entities::AgentRunId,
        error_message: &str,
    ) -> crate::error::AppResult<()> {
        self.inner.fail(id, error_message).await
    }

    async fn cancel(
        &self,
        id: &crate::domain::entities::AgentRunId,
    ) -> crate::error::AppResult<()> {
        self.inner.cancel(id).await
    }

    async fn cancel_with_reason(
        &self,
        id: &crate::domain::entities::AgentRunId,
        reason: &str,
    ) -> crate::error::AppResult<()> {
        self.inner.cancel_with_reason(id, reason).await
    }

    async fn delete(
        &self,
        id: &crate::domain::entities::AgentRunId,
    ) -> crate::error::AppResult<()> {
        self.inner.delete(id).await
    }

    async fn delete_by_conversation(
        &self,
        conversation_id: &crate::domain::entities::ChatConversationId,
    ) -> crate::error::AppResult<()> {
        self.inner.delete_by_conversation(conversation_id).await
    }

    async fn count_by_status(
        &self,
        conversation_id: &crate::domain::entities::ChatConversationId,
        status: crate::domain::entities::AgentRunStatus,
    ) -> crate::error::AppResult<u32> {
        self.inner.count_by_status(conversation_id, status).await
    }

    async fn cancel_all_running(&self) -> crate::error::AppResult<u32> {
        self.inner.cancel_all_running().await
    }

    async fn cancel_running_started_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> crate::error::AppResult<u32> {
        self.inner.cancel_running_started_before(cutoff).await
    }

    async fn get_interrupted_conversations(
        &self,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::InterruptedConversation>> {
        self.inner.get_interrupted_conversations().await
    }
}

/// Proof Obligation 5 (run-poll error path): a sustained `get_by_id` failure must not leave the
/// waiter task running forever — the wall-clock cap must still fire and exit. Idle is not evaluated
/// on this path. No `stop_agent` call because the run row is unreadable.
#[tokio::test]
async fn workspace_review_waiter_exits_and_fails_gate_when_run_poll_errors_past_wall_clock() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let inner_run_repo =
        Arc::new(crate::infrastructure::memory::MemoryAgentRunRepository::new());
    let should_fail = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut state = AppState::new_test();
    state.agent_run_repo = Arc::new(FailingAgentRunRepository {
        inner: Arc::clone(&inner_run_repo),
        should_fail: Arc::clone(&should_fail),
    });
    let state = Arc::new(state);

    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(child_conversation_id.clone());
    run.started_at = Utc::now() - chrono::Duration::seconds(3_600);
    let run_id = run.id.as_str().to_string();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("running reviewer run should persist");

    let mut reviewing_monitor = context.monitor.clone();
    apply_current_target_to_monitor(&mut reviewing_monitor, Some(&target));
    reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    reviewing_monitor.review_conversation_id = Some(child_conversation_id.clone());
    reviewing_monitor.last_run_id = Some(run_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor)
        .await
        .expect("reviewing monitor should persist");

    // All get_by_id calls now fail — this is the sustained read failure the fold-in fixes.
    should_fail.store(true, Ordering::SeqCst);

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&state),
        workspace.clone(),
        target.clone(),
        run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_secs(60),
            max_wall_clock: Duration::from_millis(150),
            completion_grace: Duration::from_millis(50),
        },
        Arc::clone(&chat_service),
    );
    handle.await.expect("waiter should complete");

    let monitor = read_monitor(&state, &workspace).await;
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_ERR_TIMED_OUT_NO_REVIEW)
    );
    assert!(
        chat_service.get_stop_agent_calls().await.is_empty(),
        "stop_agent must not be called when the run row is unreadable"
    );
}

/// Wraps the memory workspace repo and fails `get_workspace_review_monitor` while `should_fail`
/// is set, letting setup writes through before the flag is raised.
struct FailingMonitorReadRepository {
    inner: Arc<crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository>,
    should_fail: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl crate::domain::repositories::AgentConversationWorkspaceRepository
    for FailingMonitorReadRepository
{
    async fn create_or_update(
        &self,
        workspace: crate::domain::entities::AgentConversationWorkspace,
    ) -> crate::error::AppResult<crate::domain::entities::AgentConversationWorkspace> {
        self.inner.create_or_update(workspace).await
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<Option<crate::domain::entities::AgentConversationWorkspace>> {
        self.inner.get_by_conversation_id(conversation_id).await
    }

    async fn get_by_project_id(
        &self,
        project_id: &crate::domain::entities::ProjectId,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::AgentConversationWorkspace>> {
        self.inner.get_by_project_id(project_id).await
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::AgentConversationWorkspace>> {
        self.inner.list_active_direct_published_workspaces().await
    }

    async fn list_active_unpublished_edit_workspaces(
        &self,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::AgentConversationWorkspace>> {
        self.inner.list_active_unpublished_edit_workspaces().await
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::AgentConversationWorkspace>> {
        self.inner.list_active_needs_agent_workspaces().await
    }

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&crate::domain::entities::IdeationSessionId>,
        plan_branch_id: Option<&crate::domain::entities::PlanBranchId>,
    ) -> crate::error::AppResult<()> {
        self.inner
            .update_links(conversation_id, ideation_session_id, plan_branch_id)
            .await
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> crate::error::AppResult<()> {
        self.inner
            .update_publication(conversation_id, pr_number, pr_url, pr_status, push_status)
            .await
    }

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> crate::error::AppResult<()> {
        self.inner
            .update_pr_supervision_preferences(
                conversation_id,
                autofix_enabled,
                auto_merge_desired,
                auto_merge_method,
            )
            .await
    }

    async fn set_last_blocked_pr_health_fingerprint(
        &self,
        conversation_id: &ChatConversationId,
        fingerprint: Option<&str>,
    ) -> crate::error::AppResult<()> {
        self.inner
            .set_last_blocked_pr_health_fingerprint(conversation_id, fingerprint)
            .await
    }

    async fn set_stale_base_detected_at(
        &self,
        conversation_id: &ChatConversationId,
        detected_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> crate::error::AppResult<()> {
        self.inner
            .set_stale_base_detected_at(conversation_id, detected_at)
            .await
    }

    async fn set_review_automation_override(
        &self,
        conversation_id: &ChatConversationId,
        value: Option<bool>,
    ) -> crate::error::AppResult<()> {
        self.inner
            .set_review_automation_override(conversation_id, value)
            .await
    }

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: crate::domain::entities::AgentConversationWorkspaceStatus,
    ) -> crate::error::AppResult<()> {
        self.inner.update_status(conversation_id, status).await
    }

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: crate::domain::entities::AgentWorkspacePrDescription,
    ) -> crate::error::AppResult<()> {
        self.inner
            .save_pr_description(conversation_id, description)
            .await
    }

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<Option<crate::domain::entities::AgentWorkspacePrDescription>> {
        self.inner.get_pr_description(conversation_id).await
    }

    async fn clear_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<()> {
        self.inner.clear_pr_description(conversation_id).await
    }

    async fn append_publication_event(
        &self,
        event: crate::domain::entities::AgentConversationWorkspacePublicationEvent,
    ) -> crate::error::AppResult<()> {
        self.inner.append_publication_event(event).await
    }

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<
        Vec<crate::domain::entities::AgentConversationWorkspacePublicationEvent>,
    > {
        self.inner.list_publication_events(conversation_id).await
    }

    async fn set_pr_review_auto_approve_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> crate::error::AppResult<crate::domain::entities::AgentWorkspacePrReviewMonitor> {
        self.inner
            .set_pr_review_auto_approve_enabled(conversation_id, enabled)
            .await
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<crate::domain::entities::AgentWorkspacePrReviewMonitor> {
        self.inner
            .mark_pr_review_first_action_resolved(conversation_id)
            .await
    }

    async fn claim_pending_pr_review_action(
        &self,
        action_id: &str,
    ) -> crate::error::AppResult<bool> {
        self.inner.claim_pending_pr_review_action(action_id).await
    }

    async fn delete(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<()> {
        self.inner.delete(conversation_id).await
    }

    async fn upsert_workspace_review_monitor(
        &self,
        monitor: crate::domain::entities::AgentWorkspaceReviewMonitor,
    ) -> crate::error::AppResult<crate::domain::entities::AgentWorkspaceReviewMonitor> {
        self.inner.upsert_workspace_review_monitor(monitor).await
    }

    async fn get_workspace_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<Option<crate::domain::entities::AgentWorkspaceReviewMonitor>> {
        if self
            .should_fail
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            Err(crate::error::AppError::Infrastructure(
                "simulated monitor read failure".to_string(),
            ))
        } else {
            self.inner.get_workspace_review_monitor(conversation_id).await
        }
    }
}

/// Proof Obligation 11: when every durable monitor read fails throughout the grace window, the
/// settlement selects `WORKSPACE_REVIEW_ERR_UNVERIFIABLE_REVIEW` (not the "no review" variant).
/// `mark_workspace_review_blocked` silently fails because it also reads the monitor, so we verify
/// the `Failed` path executed by asserting that `stop_workspace_review_child_after_block` ran.
#[tokio::test]
async fn workspace_review_waiter_records_unverifiable_error_when_monitor_is_unreadable_during_grace(
) {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let inner_workspace_repo = Arc::new(
        crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository::new(),
    );
    let should_fail = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut state = AppState::new_test();
    state.agent_conversation_workspace_repo = Arc::new(FailingMonitorReadRepository {
        inner: Arc::clone(&inner_workspace_repo),
        should_fail: Arc::clone(&should_fail),
    });
    let state = Arc::new(state);

    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(child_conversation_id.clone());
    run.started_at = Utc::now() - chrono::Duration::seconds(3_600);
    let run_id = run.id.as_str().to_string();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("running reviewer run should persist");

    let mut reviewing_monitor = context.monitor.clone();
    apply_current_target_to_monitor(&mut reviewing_monitor, Some(&target));
    reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    reviewing_monitor.review_conversation_id = Some(child_conversation_id.clone());
    reviewing_monitor.last_run_id = Some(run_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor)
        .await
        .expect("reviewing monitor should persist");

    // From this point every get_workspace_review_monitor call returns Err.
    should_fail.store(true, Ordering::SeqCst);

    let chat_service = Arc::new(MockChatService::new());
    let handle = spawn_workspace_review_waiter_with_chat_service(
        Arc::clone(&state),
        workspace.clone(),
        target.clone(),
        run_id.clone(),
        WorkspaceReviewWaiterDeadlines {
            idle_timeout: Duration::from_millis(100),
            max_wall_clock: Duration::from_secs(60),
            completion_grace: Duration::from_millis(50),
        },
        Arc::clone(&chat_service),
    );
    handle.await.expect("waiter should complete");

    // stop_workspace_review_child_after_block is called after Failed settlement regardless of
    // whether mark_workspace_review_blocked persisted (it cannot when the monitor is unreadable).
    assert_eq!(
        chat_service.get_stop_agent_calls().await,
        vec![(
            ChatContextType::Project,
            child_conversation_id.as_str().to_owned()
        )]
    );
}

#[tokio::test]
async fn startup_reconciliation_blocks_cancelled_workspace_review_monitor() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut cancelled_run = AgentRun::new(child_conversation_id.clone());
    let run_id = cancelled_run.id.as_str().to_string();
    cancelled_run.cancel();
    cancelled_run.error_message =
        Some(crate::domain::repositories::ORPHANED_AGENT_RUN_ON_APP_RESTART.to_string());
    state
        .agent_run_repo
        .create(cancelled_run)
        .await
        .expect("cancelled run should persist");

    let mut monitor = context.monitor;
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(child_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");

    let reconciled = reconcile_interrupted_agent_workspace_reviews_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
    )
    .await
    .expect("startup reconciliation should succeed");

    assert_eq!(reconciled, 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(monitor.last_run_id.as_deref(), Some(run_id.as_str()));
    assert_eq!(
        monitor.last_error.as_deref(),
        Some("Workspace reviewer was interrupted when the app restarted")
    );
}

#[tokio::test]
async fn startup_fixer_reconciliation_reconnects_exact_action_run() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let attempt_id = "fixer-attempt-running";
    let monitor = fixer_attempt_monitor(
        conversation_id.clone(),
        ProjectId("project-1".to_string()),
        attempt_id,
        WORKSPACE_REVIEW_FIXER_STATUS_ROUTING,
    );
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("routing monitor should persist");

    let mut run = AgentRun::new(conversation_id.clone());
    let run_id = run.id.as_str().to_string();
    run.apply_action_metadata_json(Some(
        &serde_json::json!({
            "ralphx_action_kind": "workspace_review_fixer",
            "ralphx_action_context_id": conversation_id.as_str(),
            "ralphx_action_target_id": attempt_id,
        })
        .to_string(),
    ));
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("action run should persist");

    let reconciled = reconcile_interrupted_workspace_review_fixers_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.queued_message_repo),
    )
    .await
    .expect("fixer reconciliation should succeed");

    assert_eq!(reconciled, 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(
        monitor.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_RUNNING)
    );
    assert_eq!(
        monitor.review_fixer_run_id.as_deref(),
        Some(run_id.as_str())
    );
    assert_eq!(monitor.review_fixer_attempt_id.as_deref(), Some(attempt_id));
}

#[tokio::test]
async fn startup_fixer_reconciliation_fails_orphan_but_preserves_exact_queue() {
    let state = AppState::new_test();
    let project_id = ProjectId("project-1".to_string());
    let queued_conversation_id = ChatConversationId::new();
    let orphan_conversation_id = ChatConversationId::new();
    for (conversation_id, attempt_id) in [
        (&queued_conversation_id, "fixer-attempt-queued"),
        (&orphan_conversation_id, "fixer-attempt-orphan"),
    ] {
        let monitor = fixer_attempt_monitor(
            conversation_id.clone(),
            project_id.clone(),
            attempt_id,
            WORKSPACE_REVIEW_FIXER_STATUS_ROUTING,
        );
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("routing monitor should persist");
    }
    let mut queued = QueuedMessage::new("repair".to_string());
    queued.metadata_override = Some(
        serde_json::json!({
            "ralphx_action_kind": "workspace_review_fixer",
            "ralphx_action_context_id": queued_conversation_id.as_str(),
            "ralphx_action_target_id": "fixer-attempt-queued",
        })
        .to_string(),
    );
    state
        .queued_message_repo
        .enqueue_back(&QueueKey::project(project_id.as_str()), &queued)
        .await
        .expect("queued repair should persist");

    let reconciled = reconcile_interrupted_workspace_review_fixers_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.queued_message_repo),
    )
    .await
    .expect("fixer reconciliation should succeed");

    assert_eq!(reconciled, 2);
    let queued_monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&queued_conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        queued_monitor.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_QUEUED)
    );
    let orphan_monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&orphan_conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        orphan_monitor.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
    );
    assert_eq!(
        orphan_monitor.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_INTERRUPTED_ON_STARTUP_ERROR)
    );
}

#[tokio::test]
async fn startup_fixer_reconciliation_fails_orphaned_queued_and_running_attempts() {
    let state = AppState::new_test();
    let project_id = ProjectId("project-1".to_string());
    let queued_conversation_id = ChatConversationId::new();
    let running_conversation_id = ChatConversationId::new();
    for (conversation_id, attempt_id, status) in [
        (
            &queued_conversation_id,
            "fixer-attempt-queued-orphan",
            WORKSPACE_REVIEW_FIXER_STATUS_QUEUED,
        ),
        (
            &running_conversation_id,
            "fixer-attempt-running-orphan",
            WORKSPACE_REVIEW_FIXER_STATUS_RUNNING,
        ),
    ] {
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(fixer_attempt_monitor(
                conversation_id.clone(),
                project_id.clone(),
                attempt_id,
                status,
            ))
            .await
            .expect("active fixer monitor should persist");
    }

    let reconciled = reconcile_interrupted_workspace_review_fixers_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.queued_message_repo),
    )
    .await
    .expect("active orphan recovery should succeed");

    assert_eq!(reconciled, 2);
    for conversation_id in [queued_conversation_id, running_conversation_id] {
        let monitor = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            monitor.review_fixer_status.as_deref(),
            Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
        );
        assert_eq!(
            monitor.last_error.as_deref(),
            Some(WORKSPACE_REVIEW_FIXER_INTERRUPTED_ON_STARTUP_ERROR)
        );
    }
}

#[tokio::test]
async fn startup_fixer_reconciliation_fails_malformed_attempt_and_continues() {
    let state = AppState::new_test();
    let project_id = ProjectId("project-1".to_string());
    let valid_conversation_id = ChatConversationId::new();
    let valid_attempt_id = "fixer-attempt-valid-after-malformed";
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(fixer_attempt_monitor(
            valid_conversation_id.clone(),
            project_id.clone(),
            valid_attempt_id,
            WORKSPACE_REVIEW_FIXER_STATUS_ROUTING,
        ))
        .await
        .unwrap();
    let mut run = AgentRun::new(valid_conversation_id.clone());
    run.apply_action_metadata_json(Some(
        &serde_json::json!({
            "ralphx_action_kind": "workspace_review_fixer",
            "ralphx_action_context_id": valid_conversation_id.as_str(),
            "ralphx_action_target_id": valid_attempt_id,
        })
        .to_string(),
    ));
    state.agent_run_repo.create(run).await.unwrap();

    let malformed_conversation_id = ChatConversationId::new();
    let mut malformed =
        AgentWorkspaceReviewMonitor::new(malformed_conversation_id.clone(), project_id);
    malformed.review_fixer_status = Some(WORKSPACE_REVIEW_FIXER_STATUS_ROUTING.to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(malformed)
        .await
        .unwrap();

    let reconciled = reconcile_interrupted_workspace_review_fixers_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.queued_message_repo),
    )
    .await
    .expect("one malformed attempt must not block later recovery");

    assert_eq!(reconciled, 2);
    let malformed = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&malformed_conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        malformed.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
    );
    assert_eq!(
        malformed.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_INVALID_AUTHORITY_ON_STARTUP_ERROR)
    );
    let valid = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&valid_conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        valid.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_RUNNING)
    );
}

#[tokio::test]
async fn startup_reconciliation_ignores_still_running_workspace_review_monitor() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let running_run = AgentRun::new(child_conversation_id.clone());
    let run_id = running_run.id.as_str().to_string();
    state
        .agent_run_repo
        .create(running_run)
        .await
        .expect("running run should persist");

    let mut monitor = context.monitor;
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(child_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");

    let reconciled = reconcile_interrupted_agent_workspace_reviews_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
    )
    .await
    .expect("startup reconciliation should succeed");

    assert_eq!(reconciled, 0);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Reviewing);
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Reviewing
    );
    assert_eq!(monitor.last_run_id.as_deref(), Some(run_id.as_str()));
    assert_eq!(monitor.last_error, None);
}

#[tokio::test]
async fn startup_reconciliation_marks_completed_current_workspace_review_ready() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut completed_run = AgentRun::new(child_conversation_id.clone());
    let run_id = completed_run.id.as_str().to_string();
    completed_run.complete();
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("completed run should persist");

    let mut monitor = context.monitor;
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(child_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(run_id.clone()),
        ArtifactId::from_string("artifact-startup-ready"),
        9,
        Utc::now(),
        None,
        ArtifactId::from_string("artifact-startup-ready-requested-changes"),
        9,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");

    let reconciled = reconcile_interrupted_agent_workspace_reviews_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
    )
    .await
    .expect("startup reconciliation should succeed");

    assert_eq!(reconciled, 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(monitor.review_outcome, AgentWorkspaceReviewOutcome::Passed);
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
    assert_eq!(monitor.review_artifact_version, Some(9));
    assert_eq!(monitor.last_error, None);
}

#[tokio::test]
async fn startup_reconciliation_does_not_consume_completed_review_output_in_plan_mode() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("Edit context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut completed_run = AgentRun::new(child_conversation_id.clone());
    let run_id = completed_run.id.as_str().to_string();
    completed_run.complete();
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("completed run should persist");

    let mut monitor = context.monitor;
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(child_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(run_id),
        ArtifactId::from_string("historical-plan-review-artifact"),
        7,
        Utc::now(),
        None,
        ArtifactId::from_string("review-requested-changes-1"),
        7,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("PLAN workspace should persist");

    assert_eq!(
        reconcile_interrupted_agent_workspace_reviews_on_startup(
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.agent_run_repo),
        )
        .await
        .expect("PLAN startup reconciliation should perform cleanup"),
        1
    );
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(monitor.review_artifact_version, Some(7));
    assert!(monitor
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("mode changed to Plan")));
}

#[tokio::test]
async fn startup_reconciliation_blocks_completed_stale_workspace_review_artifact() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut completed_run = AgentRun::new(child_conversation_id.clone());
    let run_id = completed_run.id.as_str().to_string();
    completed_run.complete();
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("completed run should persist");

    let mut monitor = context.monitor;
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        "stale-diff-fingerprint".to_string(),
        Some(run_id.clone()),
        ArtifactId::from_string("artifact-startup-stale"),
        8,
        Utc::now(),
        None,
        ArtifactId::from_string("artifact-startup-stale-requested-changes"),
        8,
        Utc::now(),
        None,
    );
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(child_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");

    let reconciled = reconcile_interrupted_agent_workspace_reviews_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
    )
    .await
    .expect("startup reconciliation should succeed");

    assert_eq!(reconciled, 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(
        monitor.last_error.as_deref(),
        Some("Workspace reviewer completed without writing a current Review")
    );
}

#[tokio::test]
async fn startup_reconciliation_preserves_completed_current_artifact_without_outcome() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut completed_run = AgentRun::new(child_conversation_id.clone());
    let run_id = completed_run.id.as_str().to_string();
    completed_run.complete();
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("completed run should persist");

    let mut monitor = context.monitor;
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(child_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(run_id.clone()),
        ArtifactId::from_string("artifact-startup-unfinalized"),
        10,
        Utc::now(),
        None,
    );
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");

    let reconciled = reconcile_interrupted_agent_workspace_reviews_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
    )
    .await
    .expect("startup reconciliation should succeed");

    assert_eq!(reconciled, 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(monitor.review_outcome, AgentWorkspaceReviewOutcome::None);
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Required
    );
    assert_eq!(monitor.review_artifact_version, Some(10));
    assert_eq!(monitor.last_error, None);
}

#[tokio::test]
async fn startup_reconciliation_preserves_completed_run_failed_current_artifact_error() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");

    let child_conversation_id = ChatConversationId::new();
    let mut completed_run = AgentRun::new(child_conversation_id.clone());
    let run_id = completed_run.id.as_str().to_string();
    completed_run.complete();
    state
        .agent_run_repo
        .create(completed_run)
        .await
        .expect("completed run should persist");

    let mut monitor = context.monitor;
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some(run_id.clone()),
        ArtifactId::from_string("artifact-startup-run-failed"),
        11,
        Utc::now(),
        None,
    );
    let specific_error = "Workspace review packet requires additional hunk annotations".to_string();
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
    monitor.review_conversation_id = Some(child_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    monitor.last_error = Some(specific_error.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");

    let reconciled = reconcile_interrupted_agent_workspace_reviews_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
    )
    .await
    .expect("startup reconciliation should succeed");

    assert_eq!(reconciled, 1);
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(monitor.review_artifact_version, Some(11));
    assert_eq!(monitor.last_error.as_deref(), Some(specific_error.as_str()));
}

#[tokio::test]
async fn complete_review_run_sets_typed_outcome_and_gate_statuses() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let mut failed_monitor = context.monitor;
    failed_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    failed_monitor.last_run_id = Some("run-failed".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(failed_monitor)
        .await
        .expect("failed active monitor should persist");
    let failed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("run_failed".to_string()),
        Some("review failed".to_string()),
        None,
        Some("run-failed".to_string()),
    )
    .await
    .expect("failed completion should persist");
    assert_eq!(failed.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        failed.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        failed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(failed.last_run_id.as_deref(), Some("run-failed"));

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut ready_monitor = context.monitor;
    ready_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    apply_review_artifact_to_monitor(
        &mut ready_monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-ready".to_string()),
        ArtifactId::from_string("artifact-ready"),
        3,
        Utc::now(),
        None,
    );
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(ready_monitor)
        .await
        .expect("ready monitor should persist");
    let ready = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("passed".to_string()),
        Some("No blocking findings".to_string()),
        None,
        Some("run-ready".to_string()),
    )
    .await
    .expect("ready completion should persist");
    assert_eq!(ready.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(ready.review_outcome, AgentWorkspaceReviewOutcome::Passed);
    assert_eq!(
        ready.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
    assert_eq!(ready.review_artifact_version, Some(3));

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut blocked_monitor = context.monitor;
    blocked_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    apply_review_artifact_to_monitor(
        &mut blocked_monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-blocked".to_string()),
        ArtifactId::from_string("artifact-blocked"),
        4,
        Utc::now(),
        None,
    );
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(blocked_monitor)
        .await
        .expect("blocked active monitor should persist");
    let blocked = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Blocking issue summary".to_string()),
        None,
        Some("run-blocked".to_string()),
    )
    .await
    .expect("blocked completion should persist");
    assert_eq!(blocked.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(
        blocked.review_outcome,
        AgentWorkspaceReviewOutcome::Blocking
    );
    assert_eq!(
        blocked.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert_eq!(blocked.last_run_id.as_deref(), Some("run-blocked"));
    assert_eq!(
        blocked.review_blocking_summary.as_deref(),
        Some("Blocking issue summary")
    );
    assert!(blocked.review_blocking_fingerprint.is_some());
    assert_eq!(blocked.review_fixer_status.as_deref(), Some("failed"));
    assert!(blocked.review_fixer_run_id.is_none());
    assert!(blocked.review_fixer_conversation_id.is_none());
    assert!(blocked
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("Failed to route Review fixer")));
}

#[tokio::test]
async fn complete_blocking_review_keeps_gate_blocking_when_cycle_cap_is_reached() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            workspace_review_fixer_cycle_cap: 0,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should persist");
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-current"),
        1,
        Utc::now(),
        None,
    );
    monitor.review_blocking_fingerprint = Some("stale-blocking-fingerprint".to_string());
    monitor.review_fixer_status = Some(WORKSPACE_REVIEW_FIXER_STATUS_RUNNING.to_string());
    monitor.review_fixer_run_id = Some("stale-fixer-run".to_string());
    monitor.review_fixer_conversation_id =
        Some(ChatConversationId::from_string("stale-fixer-conversation"));
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("ready monitor should persist");

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("New blocking issue".to_string()),
        None,
        Some("review-run".to_string()),
    )
    .await
    .expect("blocking completion should persist without auto-routing");

    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert_eq!(
        completed.review_outcome,
        AgentWorkspaceReviewOutcome::Blocking
    );
    assert_eq!(
        completed.review_blocking_summary.as_deref(),
        Some("New blocking issue")
    );
    assert!(completed.review_blocking_fingerprint.is_some());
    assert_ne!(
        completed.review_blocking_fingerprint.as_deref(),
        Some("stale-blocking-fingerprint")
    );
    assert_eq!(
        completed.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED)
    );
    assert!(completed.review_fixer_attempt_id.is_none());
    assert_eq!(completed.review_fixer_cycle_count, 0);
    assert!(completed.review_fixer_run_id.is_none());
    assert!(completed.review_fixer_conversation_id.is_none());
    assert!(completed.last_error.is_none());
}

#[tokio::test]
async fn complete_blocking_review_does_not_autoroute_fixer_when_autofix_is_disabled() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            autofix_workspace_review_blocking_findings: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should persist");
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    persist_active_review_for_current_target(
        &state,
        &workspace,
        "review-autofix-disabled",
        "artifact-autofix-disabled",
        0,
    )
    .await;

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Manual repair is required.".to_string()),
        None,
        Some("review-autofix-disabled".to_string()),
    )
    .await
    .expect("blocking completion should persist without automatic routing");

    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert_eq!(completed.review_fixer_cycle_count, 0);
    assert!(completed.review_fixer_status.is_none());
    assert!(completed.review_fixer_attempt_id.is_none());
    assert!(completed.review_fixer_run_id.is_none());
    assert!(completed.review_fixer_conversation_id.is_none());
    assert!(completed.last_error.is_none());
}

#[tokio::test]
async fn review_automation_explicit_opt_out_suppresses_global_blocking_fixer() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    workspace.review_automation_override = Some(false);
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;
    persist_active_review_for_current_target(
        &state,
        &workspace,
        "review-workspace-opt-out",
        "artifact-workspace-opt-out",
        0,
    )
    .await;

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Explicit opt-out keeps repair manual.".to_string()),
        None,
        Some("review-workspace-opt-out".to_string()),
    )
    .await
    .expect("explicit opt-out should persist the blocker without routing");

    assert_eq!(completed.review_fixer_cycle_count, 0);
    assert!(completed.review_fixer_status.is_none());
    assert!(completed.review_fixer_attempt_id.is_none());
    assert!(completed.last_error.is_none());
}

#[tokio::test]
async fn review_automation_explicit_opt_in_routes_fixer_when_global_autofix_is_off() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            autofix_workspace_review_blocking_findings: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should persist");
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    workspace.review_automation_override = Some(true);
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;
    persist_active_review_for_current_target(
        &state,
        &workspace,
        "review-workspace-opt-in",
        "artifact-workspace-opt-in",
        0,
    )
    .await;

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Explicit opt-in should attempt automatic repair.".to_string()),
        None,
        Some("review-workspace-opt-in".to_string()),
    )
    .await
    .expect("explicit opt-in should route through the existing fixer path");

    assert_eq!(completed.review_fixer_cycle_count, 1);
    assert_eq!(
        completed.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
    );
    assert!(completed
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("Review fixer")));
}

#[tokio::test]
async fn review_automation_opt_in_does_not_enable_the_publish_gate() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should persist");
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    workspace.review_automation_override = Some(true);
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;

    assert_eq!(
        load_workspace_review_publish_blocker(&state, &workspace)
            .await
            .expect("publish policy should load"),
        None
    );
}

#[tokio::test]
async fn automatic_workspace_review_fixers_stop_after_the_configured_fresh_fingerprint_cap() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let mut state = AppState::new_test();
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            workspace_review_fixer_cycle_cap: 2,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should persist");
    state.agent_provider_settings_repo =
        Arc::new(crate::infrastructure::memory::MemoryAgentProviderSettingsRepository::new());
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;

    persist_active_review_for_current_target(&state, &workspace, "review-one", "artifact-one", 0)
        .await;
    let first = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("First automatic fixer finding.".to_string()),
        None,
        Some("review-one".to_string()),
    )
    .await
    .expect("first blocking completion should attempt automatic routing");
    assert_eq!(
        first.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
    );
    assert_eq!(first.review_fixer_cycle_count, 1);
    assert!(first
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("Failed to resolve Review fixer provider")));

    std::fs::write(
        repo.join("second-followup.rs"),
        "pub fn second_followup() {}\n",
    )
    .expect("second followup file should be written");
    git(&repo, &["add", "second-followup.rs"]);
    git(&repo, &["commit", "-m", "second followup change"]);
    let second_target = persist_active_review_for_current_target(
        &state,
        &workspace,
        "review-two",
        "artifact-two",
        first.review_fixer_cycle_count,
    )
    .await;
    assert_ne!(
        second_target.diff_fingerprint,
        first
            .current_diff_fingerprint
            .expect("first completion should persist a target fingerprint")
    );
    let second = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Second automatic fixer finding.".to_string()),
        None,
        Some("review-two".to_string()),
    )
    .await
    .expect("second blocking completion should attempt automatic routing");
    assert_eq!(
        second.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
    );
    assert_eq!(second.review_fixer_cycle_count, 2);
    assert!(second
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("Failed to resolve Review fixer provider")));

    commit_followup_change(&repo);
    let third_target = persist_active_review_for_current_target(
        &state,
        &workspace,
        "review-three",
        "artifact-three",
        second.review_fixer_cycle_count,
    )
    .await;
    assert_ne!(
        third_target.diff_fingerprint,
        second_target.diff_fingerprint
    );
    let capped = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Third automatic fixer finding.".to_string()),
        None,
        Some("review-three".to_string()),
    )
    .await
    .expect("capped blocking completion should persist");

    assert_eq!(
        capped.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert_eq!(
        capped.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED)
    );
    assert_eq!(capped.review_fixer_cycle_count, 2);
    assert!(capped.review_fixer_attempt_id.is_none());
    assert!(capped.review_fixer_run_id.is_none());
    assert!(capped.review_fixer_conversation_id.is_none());
    assert!(capped.last_error.is_none());
}

#[tokio::test]
async fn workspace_review_fixer_counter_survives_fresh_target_and_run_failure_then_resets_terminally(
) {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let reviewed_workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &reviewed_workspace).await;
    let first_target = persist_active_review_for_current_target(
        &state,
        &reviewed_workspace,
        "review-old-target",
        "artifact-old-target",
        2,
    )
    .await;

    commit_followup_change(&repo);
    let failed = complete_agent_workspace_review_run(
        &state,
        &reviewed_workspace,
        Some("run_failed".to_string()),
        Some("Reviewer process failed after the target changed.".to_string()),
        None,
        Some("review-old-target".to_string()),
    )
    .await
    .expect("run failure should persist after clearing the stale target");
    assert_eq!(failed.review_fixer_cycle_count, 2);
    assert_ne!(
        failed.current_diff_fingerprint.as_deref(),
        Some(first_target.diff_fingerprint.as_str())
    );
    assert_eq!(
        failed.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );

    persist_active_review_for_current_target(
        &state,
        &reviewed_workspace,
        "review-pass-reset",
        "artifact-pass-reset",
        failed.review_fixer_cycle_count,
    )
    .await;
    let passed = complete_agent_workspace_review_run(
        &state,
        &reviewed_workspace,
        Some("passed".to_string()),
        Some("No blockers remain.".to_string()),
        None,
        Some("review-pass-reset".to_string()),
    )
    .await
    .expect("passing completion should reset the fixer cycle");
    assert_eq!(passed.review_fixer_cycle_count, 0);

    let mut no_target = AgentWorkspaceReviewMonitor::new(
        reviewed_workspace.conversation_id.clone(),
        reviewed_workspace.project_id.clone(),
    );
    no_target.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    no_target.review_fixer_status = Some(WORKSPACE_REVIEW_FIXER_STATUS_RUNNING.to_string());
    no_target.review_fixer_attempt_id = Some("stale-no-target-attempt".to_string());
    no_target.review_fixer_cycle_count = 4;
    apply_review_gate_to_monitor(&mut no_target, None);

    assert_eq!(no_target.review_fixer_cycle_count, 0);
    assert_eq!(
        no_target.review_gate_status,
        AgentWorkspaceReviewGateStatus::NotRequired
    );
    assert!(no_target.review_fixer_status.is_none());
    assert!(no_target.review_fixer_attempt_id.is_none());
}

#[tokio::test]
async fn automatic_workspace_review_fixer_fails_closed_when_settings_cannot_be_read() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let mut state = AppState::new_test();
    state.review_settings_repo = Arc::new(FailingReviewSettingsRepository);
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    persist_active_review_for_current_target(
        &state,
        &workspace,
        "review-settings-failure",
        "artifact-settings-failure",
        1,
    )
    .await;

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Settings read must not authorize a fixer.".to_string()),
        None,
        Some("review-settings-failure".to_string()),
    )
    .await
    .expect("blocking completion should fail closed rather than fail the review");

    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert_eq!(completed.review_fixer_cycle_count, 1);
    assert!(completed.review_fixer_status.is_none());
    assert!(completed.review_fixer_attempt_id.is_none());
    assert!(completed.review_fixer_run_id.is_none());
    assert!(completed.review_fixer_conversation_id.is_none());
    assert!(completed.last_error.is_none());
}

#[tokio::test]
async fn review_automation_explicit_opt_in_uses_default_cap_when_settings_are_unavailable() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let mut state = AppState::new_test();
    state.review_settings_repo = Arc::new(FailingReviewSettingsRepository);
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    workspace.review_automation_override = Some(true);
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;
    persist_active_review_for_current_target(
        &state,
        &workspace,
        "review-explicit-settings-failure",
        "artifact-explicit-settings-failure",
        0,
    )
    .await;

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Explicit automation survives a settings read failure.".to_string()),
        None,
        Some("review-explicit-settings-failure".to_string()),
    )
    .await
    .expect("explicit automation should remain bounded and attempt routing");

    assert_eq!(completed.review_fixer_cycle_count, 1);
    assert_eq!(
        completed.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
    );
    assert!(completed
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("Review fixer")));
}

#[tokio::test]
async fn manual_blocking_review_fixer_routes_hidden_repair_message_when_cycle_is_capped() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            workspace_review_fixer_cycle_cap: 0,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should persist");
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-current"),
        1,
        Utc::now(),
        None,
    );
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("ready monitor should persist");

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Manual fixer should still run.".to_string()),
        None,
        Some("review-run".to_string()),
    )
    .await
    .expect("blocking completion should persist");
    assert_eq!(
        completed.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED)
    );
    assert_eq!(completed.review_fixer_cycle_count, 0);
    assert!(completed.review_fixer_attempt_id.is_none());
    assert!(completed.review_fixer_run_id.is_none());
    assert!(completed.review_fixer_conversation_id.is_none());
    let blocking_fingerprint = completed
        .review_blocking_fingerprint
        .clone()
        .expect("blocking fingerprint should be recorded");

    let confirmation = WorkspaceReviewFixerConfirmation {
        target_scope: target.scope,
        diff_fingerprint: target.diff_fingerprint.clone(),
        artifact_id: completed
            .review_artifact_id
            .as_ref()
            .expect("review artifact should remain current")
            .as_str()
            .to_string(),
        artifact_version: completed
            .review_artifact_version
            .expect("review artifact version should remain current"),
        blocking_fingerprint: blocking_fingerprint.clone(),
    };
    let runtime_override = ManualRoleRuntimeOverride {
        harness: AgentHarnessKind::Claude,
        model: Some("claude-explicit-fixer".to_string()),
        effort: Some(LogicalEffort::High),
        service_tier: ManualServiceTier::Standard,
        coordination_mode: None,
        persona_id: None,
    };
    let (_timing_guard, captured_timings) = capture_workspace_review_timings();
    let start = start_agent_workspace_review_blocking_fixer_with_chat_service(
        &state,
        &workspace,
        Some(&confirmation),
        Some(&runtime_override),
        &chat_service,
    )
    .await
    .expect("manual fixer should route");
    assert_workspace_review_timing_phases(
        &captured_timings,
        "workspace_review_fixer_start_phase",
        &[
            "load_workspace",
            "load_context",
            "validate_confirmation",
            "prepare_launch",
            "resolve_runtime",
            "claim_attempt",
            "start_child_chat",
            "settle_attempt",
            "reload_context",
            "total",
        ],
    );

    assert!(start.started);
    assert_eq!(start.skipped_reason, None);
    assert_eq!(
        start.context.monitor.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_RUNNING)
    );
    assert!(start.context.monitor.review_fixer_run_id.is_some());
    assert!(start.context.monitor.review_fixer_conversation_id.is_some());
    assert_eq!(start.context.monitor.review_fixer_cycle_count, 1);

    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    let options = &sent_options[0];
    assert_eq!(
        options.conversation_id_override,
        Some(workspace.conversation_id.clone())
    );
    assert_eq!(
        options.agent_name_override.as_deref(),
        Some(agent_names::AGENT_WORKSPACE_REPAIR)
    );
    assert_eq!(
        options.runtime_source_override,
        Some(RuntimeSource::ConversationOverride)
    );
    let action = AgentRunAction::from_metadata_json(options.metadata.as_deref())
        .expect("repair send should carry typed action authority");
    assert_eq!(action.kind, AgentRunActionKind::WorkspaceReviewFixer);
    assert_eq!(action.context_id, workspace.conversation_id.as_str());
    assert_eq!(
        Some(action.target_id.as_str()),
        start.context.monitor.review_fixer_attempt_id.as_deref()
    );
    let metadata: serde_json::Value = serde_json::from_str(
        options
            .metadata
            .as_deref()
            .expect("fixer request should carry hidden message metadata"),
    )
    .expect("fixer metadata should be valid json");
    assert_eq!(metadata["hidden_from_ui"], true);
    assert_eq!(metadata["source"], "workspace_review_blocking_fixer");
    assert_eq!(
        metadata["blocking_fingerprint"].as_str(),
        Some(blocking_fingerprint.as_str())
    );

    let sent_messages = chat_service.get_sent_messages().await;
    assert_eq!(sent_messages.len(), 1);
    assert!(sent_messages[0].contains("Workspace Review found blocking issues"));
    assert!(sent_messages[0].contains("Manual fixer should still run."));
}

async fn assert_blocking_fixer_uses_enabled_default_over_stale_claude_session(
    legacy_claude_session_only: bool,
) {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state =
        AppState::new_test().with_harness_agent_client(AgentHarnessKind::Codex, codex_client);
    let provider_repo =
        Arc::new(crate::infrastructure::memory::MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    codex.model = Some("gpt-provider-default".to_string());
    codex.effort = Some(LogicalEffort::High);
    codex.approval_policy = Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string());
    codex.sandbox_mode = Some(CODEX_DEFAULT_SANDBOX_MODE.to_string());
    codex.service_tier = Some("fast".to_string());
    provider_repo.upsert(&codex).await.unwrap();
    provider_repo
        .upsert(&AgentProviderSettings::disabled_defaults(
            AgentHarnessKind::Claude,
        ))
        .await
        .unwrap();
    state.agent_provider_settings_repo = provider_repo;

    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    state
        .manual_role_default_repo
        .upsert_for_project(
            project.id.as_str(),
            RoutingRole::WorkspaceRepair,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-workspace-repair".to_string()),
                effort: Some(LogicalEffort::Medium),
                service_tier: ManualServiceTier::Standard,
                coordination_mode: None,
                persona_id: None,
                approval_policy: Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string()),
                sandbox_mode: Some(CODEX_DEFAULT_SANDBOX_MODE.to_string()),
                atlassian_access: None,
            },
        )
        .await
        .unwrap();
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.id = workspace.conversation_id.clone();
    conversation.agent_mode = Some(workspace.mode);
    if legacy_claude_session_only {
        conversation.claude_session_id = Some("legacy-claude-session".to_string());
    } else {
        conversation.set_provider_session_ref(ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: "canonical-claude-session".to_string(),
        });
    }
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let mut latest_run = AgentRun::new(workspace.conversation_id.clone());
    latest_run.harness = Some(AgentHarnessKind::Codex);
    latest_run.logical_model = Some("gpt-stale-run-model".to_string());
    latest_run.logical_effort = Some(LogicalEffort::Low);
    state
        .agent_run_repo
        .create(latest_run)
        .await
        .expect("latest Codex run should persist");

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("review-artifact-provider-fallback"),
        1,
        Utc::now(),
        None,
    );
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_blocking_summary = Some("Use the enabled provider.".to_string());
    monitor.review_blocking_fingerprint = Some(workspace_review_blocking_fingerprint(
        &target,
        "Use the enabled provider.",
    ));
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("blocking monitor should persist");

    let start = start_agent_workspace_review_blocking_fixer_with_chat_service(
        &state,
        &workspace,
        None,
        None,
        &chat_service,
    )
    .await
    .expect("blocking fixer should route through enabled default provider");

    assert!(start.started);
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    let options = &sent_options[0];
    assert_eq!(options.harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(
        options.model_override.as_deref(),
        Some("gpt-workspace-repair")
    );
    assert_eq!(options.logical_effort_override, Some(LogicalEffort::Medium));
    assert_eq!(
        options.approval_policy_override.as_deref(),
        Some(CODEX_DEFAULT_APPROVAL_POLICY)
    );
    assert_eq!(
        options.sandbox_mode_override.as_deref(),
        Some(CODEX_DEFAULT_SANDBOX_MODE)
    );
    assert_eq!(options.service_tier_override.as_deref(), Some("standard"));
    assert_eq!(
        options.runtime_source_override,
        Some(RuntimeSource::RoleDefault)
    );
    assert!(options.preserve_conversation_provider_session_ref);
    assert!(options.force_new_provider_session);
}

#[tokio::test]
async fn blocking_fixer_uses_enabled_default_over_stale_canonical_claude_session() {
    assert_blocking_fixer_uses_enabled_default_over_stale_claude_session(false).await;
}

#[tokio::test]
async fn blocking_fixer_uses_enabled_default_over_legacy_claude_session_alias() {
    assert_blocking_fixer_uses_enabled_default_over_stale_claude_session(true).await;
}

#[tokio::test]
async fn blocking_fixer_records_failed_without_sending_when_no_enabled_default_exists() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let mut state = AppState::new_test();
    state.agent_provider_settings_repo =
        Arc::new(crate::infrastructure::memory::MemoryAgentProviderSettingsRepository::new());
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("review-artifact-no-provider"),
        1,
        Utc::now(),
        None,
    );
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_blocking_summary = Some("Provider setup is required.".to_string());
    monitor.review_blocking_fingerprint = Some(workspace_review_blocking_fingerprint(
        &target,
        "Provider setup is required.",
    ));
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("blocking monitor should persist");

    let start = start_agent_workspace_review_blocking_fixer_with_chat_service(
        &state,
        &workspace,
        None,
        None,
        &chat_service,
    )
    .await
    .expect("provider resolution failure should persist a stable fixer result");

    assert!(!start.started);
    assert!(chat_service.get_sent_options().await.is_empty());
    assert_eq!(
        start.context.monitor.review_fixer_status.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_STATUS_FAILED)
    );
    assert!(start
        .context
        .monitor
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("Settings > Harness > Providers")));
}

#[tokio::test]
async fn manual_blocking_review_fixer_skips_when_fixer_already_active() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-current"),
        1,
        Utc::now(),
        None,
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.review_blocking_summary = Some("Active fixer duplicate guard.".to_string());
    monitor.review_blocking_fingerprint = Some(workspace_review_blocking_fingerprint(
        &target,
        "Active fixer duplicate guard.",
    ));
    monitor.review_fixer_status = Some(WORKSPACE_REVIEW_FIXER_STATUS_RUNNING.to_string());
    monitor.review_fixer_run_id = Some("active-fixer-run".to_string());
    monitor.review_fixer_conversation_id = Some(workspace.conversation_id.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("blocking monitor should persist");

    let start = start_agent_workspace_review_blocking_fixer_with_chat_service(
        &state,
        &workspace,
        None,
        None,
        &chat_service,
    )
    .await
    .expect("active fixer should be treated as an idempotent skip");

    assert!(!start.started);
    assert_eq!(
        start.skipped_reason.as_deref(),
        Some(WORKSPACE_REVIEW_FIXER_SKIPPED_ALREADY_ACTIVE)
    );
    assert_eq!(chat_service.get_sent_messages().await.len(), 0);
}

#[tokio::test]
async fn complete_review_run_rejects_stale_active_review_run_id() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("run-current".to_string()),
        ArtifactId::from_string("artifact-current"),
        1,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    let result = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("passed".to_string()),
        Some("No blocking findings".to_string()),
        None,
        Some("run-stale".to_string()),
    )
    .await;

    assert!(result
        .expect_err("stale run id should be rejected")
        .to_string()
        .contains("does not match the active workspace Review run"));
}

#[tokio::test]
async fn blocking_repair_message_injects_review_artifact_and_keeps_fetch_optional() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("review-artifact-1"),
        7,
        Utc::now(),
        None,
        ArtifactId::from_string("review-requested-changes-1"),
        7,
        Utc::now(),
        None,
    );
    monitor.review_blocking_summary = Some("Fix the missing review artifact access.".to_string());

    let goal_context = AgentWorkspaceReviewGoalContext {
        user_request_excerpts: vec!["Remove workspace path constraints.".to_string()],
        ..AgentWorkspaceReviewGoalContext::default()
    };
    let review_artifact_context = AgentWorkspaceReviewResolvedArtifactContext {
        artifact_id: "review-artifact-1".to_string(),
        kind: "review".to_string(),
        title: Some("Workspace Review".to_string()),
        session_id: None,
        version: Some(7),
        content: "## Summary\n\nBlocking detail from generated Review.".to_string(),
        content_truncated: false,
        original_chars: 49,
    };
    let requested_changes_artifact_context = AgentWorkspaceReviewResolvedArtifactContext {
        artifact_id: "review-requested-changes-1".to_string(),
        kind: "review_requested_changes".to_string(),
        title: Some("Workspace Review — Requested Changes".to_string()),
        session_id: None,
        version: Some(7),
        content: "## Step 1\n\nUpdate the exact repair seam.".to_string(),
        content_truncated: false,
        original_chars: 45,
    };
    let message = build_workspace_review_blocking_repair_message(
        &workspace,
        &monitor,
        &target,
        &goal_context,
        Some(&review_artifact_context),
        Some(&requested_changes_artifact_context),
    );

    assert!(message.contains("Review artifact: review-artifact-1 v7"));
    assert!(message.contains("Requested Changes artifact: review-requested-changes-1 v7"));
    assert!(message.contains("Review Overview content injected by RalphX"));
    assert!(message.contains("Requested Changes content injected by RalphX"));
    assert!(message.contains("Blocking detail from generated Review."));
    assert!(message.contains("Update the exact repair seam."));
    assert!(message.contains(
        "Call `get_artifact` only if this injected content is truncated or insufficient."
    ));
    assert!(!message.contains("Fetch the full Review artifact before editing"));
    assert!(message.contains("<workspace_goal_context>"));
    assert!(message.contains("Remove workspace path constraints."));
    assert!(message.contains("Fix the missing review artifact access."));
    assert!(message.contains("call `complete_agent_workspace_repair` with a concise summary"));
    assert!(message.contains("summary and blocker"));
    for transport_owned_detail in [
        "Conversation ID:",
        "Review target scope:",
        "Review diff fingerprint:",
        "Review child conversation:",
        "Review run ID:",
        "repair commit SHA",
        "resolved base ref",
        "resolved base commit",
        "attempt ID",
        "orchestration ID",
        "timestamp",
        "rescue",
    ] {
        assert!(
            !message.contains(transport_owned_detail),
            "repair prompt must not request or expose transport-owned detail: {transport_owned_detail}"
        );
    }
}

#[tokio::test]
async fn blocking_repair_send_inherits_parent_associated_references_for_expansion() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let chat_service = MockChatService::new();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let mut plan_artifact = Artifact::new_inline(
        "Approved parent plan",
        ArtifactType::Specification,
        "# Plan\n\nKeep parent references available to child repair.",
        "ralphx-ideation",
    );
    plan_artifact.metadata.version = 3;
    let plan_artifact = state
        .artifact_repo
        .create(plan_artifact)
        .await
        .expect("plan artifact should persist");
    let blueprint_artifact = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Parent implementation blueprint",
            ArtifactType::Specification,
            "# Blueprint\n\nKeep parent references available to child repair.",
            "ralphx-ideation",
        ))
        .await
        .expect("blueprint artifact should persist");
    let planning_session = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .plan_artifact_id(plan_artifact.id.clone())
        .plan_blueprint_artifact_id(blueprint_artifact.id.clone())
        .build();
    let planning_session = state
        .ideation_session_repo
        .create(planning_session)
        .await
        .expect("planning session should persist");
    workspace.linked_ideation_session_id = Some(planning_session.id.clone());
    let mut parent_message =
        ChatMessage::user_in_project(project.id.clone(), "Repair the reviewed implementation");
    parent_message.conversation_id = Some(workspace.conversation_id.clone());
    parent_message.metadata = Some(
        serde_json::json!({
            "composer_artifact_references": [
                { "artifactId": "design-1", "kind": "design" },
                { "artifactId": "design-2", "kind": "design" },
                { "artifactId": "design-3", "kind": "design" },
                { "artifactId": "design-4", "kind": "design" },
                { "artifactId": "design-5", "kind": "design" },
                { "artifactId": "design-6", "kind": "design" },
                { "artifactId": "design-7", "kind": "design" },
                { "artifactId": "design-8", "kind": "design" }
            ]
        })
        .to_string(),
    );
    state
        .chat_message_repo
        .create(parent_message)
        .await
        .expect("parent goal references should persist");

    state
        .agent_conversation_jira_issue_repo
        .upsert(
            AgentConversationJiraIssueLink::new(
                workspace.conversation_id.clone(),
                project.id.clone(),
                "RX-42".to_string(),
                Utc::now(),
            )
            .with_reference_metadata(
                Some("jira-42".to_string()),
                Some("Parent goal ticket".to_string()),
                Some("https://jira.test/browse/RX-42".to_string()),
            ),
        )
        .await
        .expect("assigned Jira issue should persist");

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor = context.monitor;
    let mut review_artifact = Artifact::new_inline(
        "Workspace Review",
        ArtifactType::ReviewFeedback,
        "## Summary\n\nPreserve parent references in the repair.",
        "ralphx-workspace-reviewer",
    );
    review_artifact.metadata.version = 1;
    let review_artifact = state
        .artifact_repo
        .create(review_artifact)
        .await
        .expect("review artifact should persist");
    let requested_changes_artifact = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Workspace Review — Requested Changes",
            ArtifactType::ReviewFeedback,
            "## Step 1\n\nPreserve parent references in the repair.",
            "ralphx-workspace-reviewer",
        ))
        .await
        .expect("requested changes artifact should persist");
    apply_review_artifact_pair_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        review_artifact.id.clone(),
        1,
        Utc::now(),
        None,
        requested_changes_artifact.id.clone(),
        1,
        Utc::now(),
        None,
    );
    monitor.review_blocking_summary = Some("Preserve parent references.".to_string());
    monitor.review_blocking_fingerprint = Some(workspace_review_blocking_fingerprint(
        &target,
        "Preserve parent references.",
    ));
    monitor.review_fixer_attempt_id = Some("fixer-attempt-1".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await
        .expect("monitor claim should persist");

    let routed = route_workspace_review_blocking_fixer_with_chat_service(
        &state,
        &workspace,
        &monitor,
        Some(&target),
        None,
        None,
        None,
        &chat_service,
    )
    .await
    .expect("blocking repair should route");

    assert_eq!(routed.review_fixer_status.as_deref(), Some("running"));
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    let options = &sent_options[0];
    assert_eq!(
        options.agent_name_override.as_deref(),
        Some(agent_names::AGENT_WORKSPACE_REPAIR)
    );
    assert_eq!(options.composer_artifact_references.len(), 8);
    assert_eq!(
        options.composer_artifact_references[0].artifact_id,
        plan_artifact.id.as_str()
    );
    assert_eq!(
        options.composer_artifact_references[1].artifact_id,
        blueprint_artifact.id.as_str()
    );
    assert_eq!(
        options.composer_artifact_references[2].artifact_id,
        review_artifact.id.as_str()
    );
    assert_eq!(
        options.composer_artifact_references[3].artifact_id,
        requested_changes_artifact.id.as_str()
    );
    assert!(options
        .composer_integration_references
        .iter()
        .any(|reference| reference.provider == "atlassian"
            && reference.kind == "jira"
            && reference.key.as_deref() == Some("RX-42")
            && reference.title.as_deref() == Some("Parent goal ticket")));
    assert!(options
        .composer_artifact_references
        .iter()
        .any(
            |reference| reference.artifact_id == plan_artifact.id.as_str()
                && reference.kind == "plan"
                && reference.session_id.as_deref() == Some(planning_session.id.as_str())
                && reference.version == Some(3)
        ));
    assert!(options
        .composer_artifact_references
        .iter()
        .any(|reference| {
            reference.artifact_id == blueprint_artifact.id.as_str()
                && reference.kind == "plan_blueprint"
                && reference.session_id.as_deref() == Some(planning_session.id.as_str())
        }));
    let metadata: serde_json::Value = serde_json::from_str(
        options
            .metadata
            .as_deref()
            .expect("fixer request should carry hidden message metadata"),
    )
    .expect("fixer metadata should be valid json");
    assert_eq!(metadata["hidden_from_ui"], true);
    assert_eq!(metadata["source"], "workspace_review_blocking_fixer");
    assert_eq!(
        metadata["blocking_fingerprint"].as_str(),
        monitor.review_blocking_fingerprint.as_deref()
    );
    assert_eq!(
        metadata["plan_context_fingerprint"].as_str(),
        monitor.reviewed_plan_context_fingerprint.as_deref()
    );

    let sent_messages = chat_service.get_sent_messages().await;
    assert_eq!(sent_messages.len(), 1);
    assert!(sent_messages[0].contains("<workspace_goal_context>"));
    assert!(sent_messages[0].contains("RX-42"));
    assert!(sent_messages[0].contains(plan_artifact.id.as_str()));
    assert!(sent_messages[0].contains("Review Overview content injected by RalphX"));
    assert!(sent_messages[0].contains("Preserve parent references in the repair."));
    assert!(!sent_messages[0].contains("Fetch the full Review artifact before editing"));

    let replacement_plan = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Replacement parent plan",
            ArtifactType::Specification,
            "# Plan\n\nThis plan changed after Review.",
            "ralphx-ideation",
        ))
        .await
        .expect("replacement plan should persist");
    let replacement_blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Replacement implementation blueprint",
            ArtifactType::Specification,
            "# Blueprint\n\nThis blueprint changed after Review.",
            "ralphx-ideation",
        ))
        .await
        .expect("replacement blueprint should persist");
    let replacement_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(workspace.conversation_id.as_str())
                .plan_artifact_id(replacement_plan.id)
                .plan_blueprint_artifact_id(replacement_blueprint.id)
                .build(),
        )
        .await
        .expect("replacement planning session should persist");
    workspace.linked_ideation_session_id = Some(replacement_session.id);
    let drift_error = ensure_workspace_review_plan_context_is_current(&state, &workspace, &monitor)
        .await
        .expect_err("fixer send must reject a plan change after preparation");
    assert!(drift_error
        .to_string()
        .contains(WORKSPACE_REVIEW_PLAN_CONTEXT_CHANGED_ERROR));
}

#[test]
fn mark_review_artifact_current_for_target_updates_reviewed_and_current_metadata() {
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        ChatConversationId::from_string("review-monitor-conversation"),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_head_sha = Some("selected-head".to_string());
    monitor.reviewed_diff_fingerprint = Some("workspace-fingerprint".to_string());
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-fingerprint".to_string());

    let target = AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        base_ref: "main".to_string(),
        base_sha: Some("base-sha".to_string()),
        head_ref: "refs/ralphx/pr-heads/483".to_string(),
        head_sha: Some("selected-head".to_string()),
        diff_fingerprint: "selected-fingerprint".to_string(),
        working_directory: PathBuf::from("/tmp/selected-source"),
        source_pull_request_number: Some(483),
        review_packet: AgentWorkspaceReviewPacket::default(),
    };

    mark_review_artifact_current_for_target(&mut monitor, &target);

    assert!(monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::SelectedSource,
        Some("selected-head"),
        "selected-fingerprint"
    ));
    assert_eq!(
        monitor.current_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );
    assert_eq!(
        monitor.current_diff_fingerprint.as_deref(),
        Some("selected-fingerprint")
    );
    assert_eq!(monitor.selected_source_pull_request_number, Some(483));
    assert_eq!(
        monitor.selected_source_head_sha.as_deref(),
        Some("selected-head")
    );
}

#[test]
fn plan_context_drift_invalidates_review_currentness_and_fixer_authority() {
    let conversation_id = ChatConversationId::from_string("plan-drift-review");
    let project_id = ProjectId::from_string("project-plan-drift".to_string());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-current".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-current".to_string());
    monitor.current_plan_context_fingerprint = Some("plan-new".to_string());
    monitor.reviewed_plan_context_fingerprint = Some("plan-reviewed".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-overview"));
    monitor.review_artifact_version = Some(1);
    monitor.review_requested_changes_artifact_id =
        Some(ArtifactId::from_string("review-requested-changes"));
    monitor.review_requested_changes_artifact_version = Some(1);
    monitor.review_blocking_fingerprint = Some("blocking-current".to_string());

    assert!(!monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        None,
        "diff-current",
    ));
    assert!(AgentWorkspaceReviewFixerSnapshot::from_monitor(&monitor).is_none());

    monitor.reviewed_head_sha = Some("reviewed-head".to_string());
    monitor.workspace_head_sha = Some("reviewed-head".to_string());
    monitor.workspace_base_sha = Some("base-head".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-head".to_string()),
        "ralphx/test/plan-drift".to_string(),
        "/tmp/plan-drift".to_string(),
    );
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some(MERGED_PUBLICATION_PR_STATUS.to_string());
    let merged_target = AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        base_ref: "main".to_string(),
        base_sha: Some("base-head".to_string()),
        head_ref: "refs/ralphx/pr-heads/483".to_string(),
        head_sha: Some("reviewed-head".to_string()),
        diff_fingerprint: "merged-target".to_string(),
        working_directory: PathBuf::from("/tmp/plan-drift"),
        source_pull_request_number: Some(483),
        review_packet: AgentWorkspaceReviewPacket::default(),
    };
    assert!(
        !workspace_review_artifact_covers_merged_pr_target(&workspace, &monitor, &merged_target),
        "merged-PR target equivalence must not bypass reviewed plan authority"
    );
}

#[tokio::test]
async fn complete_review_settles_failed_when_linked_plan_becomes_unusable() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Review plan",
            ArtifactType::Specification,
            "# Review plan",
            "planner",
        ))
        .await
        .expect("overview should persist");
    let blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Review blueprint",
            ArtifactType::Specification,
            "# Review blueprint",
            "planner",
        ))
        .await
        .expect("blueprint should persist");
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(workspace.conversation_id.as_str())
                .plan_artifact_id(overview.id)
                .plan_blueprint_artifact_id(blueprint.id)
                .build(),
        )
        .await
        .expect("planning session should persist");
    workspace.linked_ideation_session_id = Some(session.id.clone());

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("review context should load");
    let target = context.target.expect("review target should exist");
    let mut monitor = context.monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-plan-link-run".to_string()),
        ArtifactId::from_string("review-plan-link-artifact"),
        1,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("reviewing monitor should persist");
    state
        .ideation_session_repo
        .update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .expect("linked planning session should archive");

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("passed".to_string()),
        Some("No blocking findings".to_string()),
        None,
        Some("review-plan-link-run".to_string()),
    )
    .await
    .expect("broken plan authority should settle the Review instead of stranding it");

    assert_eq!(completed.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        completed.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert!(completed
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("fresh planning session")));
}

#[tokio::test]
async fn complete_review_run_carries_workspace_review_forward_after_same_pr_merges() {
    let (temp, repo, base_sha) = init_repo();
    let pr_head = committed_workspace_delta_on_branch(&repo, "feature/merged-pr");
    git(&repo, &["update-ref", "refs/ralphx/pr-heads/483", &pr_head]);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace delta context should load");
    let target = initial.target.expect("workspace delta target should exist");
    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::WorkspaceDelta
    );
    assert_eq!(target.head_sha.as_deref(), Some(pr_head.as_str()));
    let mut monitor = initial.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-merged-pr-review"),
        1,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    workspace.worktree_path = temp
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some("merged".to_string());

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("passed".to_string()),
        Some("No blocking findings".to_string()),
        None,
        Some("review-run".to_string()),
    )
    .await
    .expect("merged equivalent review should complete");

    assert_eq!(completed.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(
        completed.review_outcome,
        AgentWorkspaceReviewOutcome::Passed
    );
    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
    assert_eq!(
        completed.current_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );
    assert_eq!(
        completed.reviewed_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );
    assert_eq!(
        completed.reviewed_head_sha.as_deref(),
        Some(pr_head.as_str())
    );
    assert_eq!(completed.last_error, None);

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("merged equivalent context should load");
    assert!(context.is_current);
    assert!(!context.is_outdated);
    assert_eq!(
        context.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
}

#[tokio::test]
async fn load_context_persists_carried_merged_pr_review_for_start_skip() {
    let (temp, repo, base_sha) = init_repo();
    let pr_head = committed_workspace_delta_on_branch(&repo, "feature/merged-pr");
    git(&repo, &["update-ref", "refs/ralphx/pr-heads/483", &pr_head]);

    let state = Arc::new(AppState::new_test());
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace delta context should load");
    let target = initial.target.expect("workspace delta target should exist");
    let mut monitor = initial.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-merged-pr-review"),
        1,
        Utc::now(),
        None,
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    workspace.worktree_path = temp
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some("merged".to_string());

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("merged equivalent context should load");
    assert!(context.is_current);
    assert_eq!(
        context.monitor.reviewed_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );

    let persisted = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("persisted monitor read should succeed")
        .expect("persisted monitor should exist");
    assert_eq!(
        persisted.reviewed_target_scope,
        Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
    );
    assert_eq!(
        persisted.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );

    let chat_service = MockChatService::new();
    let start = start_agent_workspace_review_with_chat_service(
        Arc::clone(&state),
        &workspace,
        false,
        None,
        &chat_service,
    )
    .await
    .expect("current merged equivalent review should not re-run");
    assert!(!start.started);
    assert_eq!(start.skipped_reason.as_deref(), Some("current"));
    assert_eq!(chat_service.get_sent_messages().await.len(), 0);

    let persisted = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("persisted monitor read should succeed")
        .expect("persisted monitor should exist");
    assert_eq!(
        persisted.reviewed_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );
    assert_eq!(
        persisted.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
}

#[tokio::test]
async fn complete_review_run_preserves_blocking_outcome_after_same_pr_merges() {
    let (temp, repo, base_sha) = init_repo();
    let pr_head = committed_workspace_delta_on_branch(&repo, "feature/merged-pr");
    git(&repo, &["update-ref", "refs/ralphx/pr-heads/483", &pr_head]);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;
    persist_workspace(&state, &workspace).await;

    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace delta context should load");
    let target = initial.target.expect("workspace delta target should exist");
    let mut monitor = initial.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-merged-pr-blocking-review"),
        1,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    workspace.worktree_path = temp
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some("merged".to_string());
    persist_workspace(&state, &workspace).await;

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("blocking".to_string()),
        Some("Blocking issue summary".to_string()),
        None,
        Some("review-run".to_string()),
    )
    .await
    .expect("merged equivalent blocking review should complete");

    assert_eq!(completed.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(
        completed.review_outcome,
        AgentWorkspaceReviewOutcome::Blocking
    );
    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert_eq!(
        completed.reviewed_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );
    assert_eq!(
        completed.review_blocking_summary.as_deref(),
        Some("Blocking issue summary")
    );
    assert!(completed.review_blocking_fingerprint.is_some());
    assert!(completed
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("Failed to route Review fixer")));

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("merged equivalent blocking context should load");
    assert!(context.is_current);
    assert!(!context.is_outdated);
    assert_eq!(
        context.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
}

#[tokio::test]
async fn existing_merged_target_mismatch_failure_marks_context_current_without_autopass() {
    let (temp, repo, base_sha) = init_repo();
    let pr_head = committed_workspace_delta_on_branch(&repo, "feature/merged-pr");
    git(&repo, &["update-ref", "refs/ralphx/pr-heads/483", &pr_head]);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace delta context should load");
    let target = initial.target.expect("workspace delta target should exist");
    let mut monitor = initial.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-merged-pr-failed-review"),
        1,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
    monitor.last_error = Some(WORKSPACE_REVIEW_TARGET_MISMATCH_ERROR.to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    workspace.worktree_path = temp
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some("merged".to_string());

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("merged equivalent failed context should load");

    assert!(context.is_current);
    assert!(!context.is_outdated);
    assert_eq!(
        context.monitor.reviewed_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );
    assert_eq!(
        context.monitor.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        context.monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(
        context.monitor.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_TARGET_MISMATCH_ERROR)
    );
}

#[tokio::test]
async fn complete_review_run_rejects_merged_pr_when_reviewed_head_differs() {
    let (temp, repo, base_sha) = init_repo();
    let reviewed_head = committed_workspace_delta_on_branch(&repo, "feature/merged-pr");
    git(
        &repo,
        &["update-ref", "refs/ralphx/pr-heads/483", &reviewed_head],
    );

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace delta context should load");
    let target = initial.target.expect("workspace delta target should exist");
    let mut monitor = initial.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-stale-head-review"),
        1,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    let new_head = commit_followup_change(&repo);
    git(
        &repo,
        &["update-ref", "refs/ralphx/pr-heads/483", &new_head],
    );
    workspace.worktree_path = temp
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some("merged".to_string());

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("passed".to_string()),
        Some("No blocking findings".to_string()),
        None,
        Some("review-run".to_string()),
    )
    .await
    .expect("stale head completion should persist failed monitor");

    assert_eq!(completed.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        completed.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(
        completed.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_TARGET_MISMATCH_ERROR)
    );
}

#[tokio::test]
async fn complete_review_run_rejects_unmerged_pr_target_drift() {
    let (temp, repo, base_sha) = init_repo();
    let pr_head = committed_workspace_delta_on_branch(&repo, "feature/open-pr");
    git(&repo, &["update-ref", "refs/ralphx/pr-heads/483", &pr_head]);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let mut workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    seed_conversation(&state, &workspace).await;

    let initial = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace delta context should load");
    let target = initial.target.expect("workspace delta target should exist");
    let mut monitor = initial.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha.clone(),
        target.diff_fingerprint.clone(),
        Some("review-run".to_string()),
        ArtifactId::from_string("artifact-open-pr-review"),
        1,
        Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    workspace.worktree_path = temp
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();
    workspace.publication_pr_number = Some(483);
    workspace.publication_pr_status = Some("open".to_string());

    let completed = complete_agent_workspace_review_run(
        &state,
        &workspace,
        Some("passed".to_string()),
        Some("No blocking findings".to_string()),
        None,
        Some("review-run".to_string()),
    )
    .await
    .expect("open PR target drift should persist failed monitor");

    assert_eq!(completed.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        completed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(
        completed.last_error.as_deref(),
        Some(WORKSPACE_REVIEW_TARGET_MISMATCH_ERROR)
    );
}

#[tokio::test]
async fn mark_workspace_review_blocked_persists_monitor_error() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor =
        AgentWorkspaceReviewMonitor::new(workspace.conversation_id.clone(), project.id.clone());
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.last_run_id = Some("helper-1".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    mark_workspace_review_blocked(
        &state,
        &workspace,
        &target,
        "helper-1",
        "review failed".to_string(),
    )
    .await;

    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(monitor.last_run_id.as_deref(), Some("helper-1"));
    assert_eq!(monitor.last_error.as_deref(), Some("review failed"));
    assert_eq!(
        monitor.current_target_scope,
        Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
    );
}

#[tokio::test]
async fn mark_workspace_review_blocked_pauses_owning_automation() {
    use crate::domain::entities::{
        Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
        AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
        AutomationRunId, AutomationRunStatus, AutomationStatus,
    };

    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );

    // Seed an automation + run linked to this workspace's conversation.
    let now = chrono::Utc::now();
    let automation_id = AutomationId::from_string("automation-1");
    state
        .automation_repo
        .create(Automation {
            id: automation_id.clone(),
            project_id: project.id.clone(),
            name: "Automation".to_string(),
            status: AutomationStatus::Active,
            paused_reason_code: None,
            paused_reason_detail: None,
            goal_prompt: "Goal".to_string(),
            setup_conversation_id: None,
            provider_harness: "claude".to_string(),
            model_id: "sonnet".to_string(),
            logical_effort: None,
            run_mode: "edit".to_string(),
            base_ref_kind: "project_default".to_string(),
            base_ref: String::new(),
            base_display_name: None,
            base_source_pull_request_json: None,
            goal_items_json: None,
            chain_mode: "merged_base".to_string(),
            completion_signal: "pr_merged".to_string(),
            plan_approval_mode: AutomationPlanApprovalMode::Manual,
            pr_merge_mode: AutomationPrMergeMode::Manual,
            plan_deep_verification: false,
            max_runs: 25,
            max_consecutive_failures: 3,
            first_run_prompt: None,
            setup_analysis_summary: None,
            spec_artifact_id: None,
            authoring_state_json: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    let run_id = AutomationRunId::from_string("run-1");
    state
        .automation_run_repo
        .create_run(AutomationRun {
            id: run_id.clone(),
            automation_id: automation_id.clone(),
            run_index: 1,
            status: AutomationRunStatus::Running,
            judge_state: AutomationJudgeState::None,
            judge_lease_expires_at: None,
            plan_judge_state: AutomationPlanJudgeState::None,
            plan_judge_lease_expires_at: None,
            plan_judge_verdict_json: None,
            plan_revision_round: 0,
            plan_reminder_count: 0,
            plan_pending_instructions: None,
            plan_last_parked_artifact_id: None,
            plan_last_parked_blueprint_artifact_id: None,
            agent_phase_started_at: None,
            conversation_id: Some(workspace.conversation_id.clone()),
            run_prompt: "Run".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: "main".to_string(),
            base_from_run_id: None,
            goal_item_id: None,
            branch_name: None,
            pr_number: None,
            pr_url: None,
            pr_title: None,
            pr_head_ref_name: None,
            pr_base_ref_name: None,
            pr_merged_at: None,
            merge_commit_sha: None,
            diff_stats_json: None,
            agent_summary: None,
            judge_verdict_json: None,
            judge_model_id: None,
            error_code: None,
            error_detail: None,
            signal_check_failures: 0,
            started_at: Some(now),
            finished_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut monitor =
        AgentWorkspaceReviewMonitor::new(workspace.conversation_id.clone(), project.id.clone());
    apply_current_target_to_monitor(&mut monitor, Some(&target));
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.last_run_id = Some("helper-1".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    mark_workspace_review_blocked(
        &state,
        &workspace,
        &target,
        "helper-1",
        "review failed".to_string(),
    )
    .await;

    let paused = state
        .automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, AutomationStatus::Paused);
    assert_eq!(
        paused.paused_reason_code.as_deref(),
        Some("workspace_review_blocked")
    );
    let terminal_run = state
        .automation_run_repo
        .get_by_id(&run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(terminal_run.status, AutomationRunStatus::AgentFailed);
    assert_eq!(
        terminal_run.error_code.as_deref(),
        Some("workspace_review_blocked")
    );
}

#[tokio::test]
async fn stale_workspace_review_block_does_not_clobber_newer_review() {
    let (_temp, repo, base_sha) = init_repo();
    committed_workspace_delta(&repo);

    let state = AppState::new_test();
    let project = seed_project(&state, &repo).await;
    let workspace = workspace(
        &project,
        &repo,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main",
        Some(base_sha),
    );
    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("context should load");
    let target = context.target.expect("target should exist");
    let mut reviewing_monitor = context.monitor;
    reviewing_monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    reviewing_monitor.last_run_id = Some("new-run".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor)
        .await
        .expect("reviewing monitor should persist");

    mark_workspace_review_blocked(
        &state,
        &workspace,
        &target,
        "old-run",
        "old run failed".to_string(),
    )
    .await;

    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("monitor should exist");
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Reviewing);
    assert_eq!(monitor.last_run_id.as_deref(), Some("new-run"));
    assert_eq!(monitor.last_error, None);
    assert_eq!(
        monitor.current_diff_fingerprint.as_deref(),
        Some(target.diff_fingerprint.as_str())
    );
}

#[test]
fn review_request_message_and_started_summary_describe_targets() {
    let project_id = crate::domain::entities::ProjectId::new();
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-review-message"),
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "feature/review".to_string(),
        "/tmp/worktree".to_string(),
    );
    let selected = AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        base_ref: "main".to_string(),
        base_sha: Some("base-sha".to_string()),
        head_ref: "feature/review".to_string(),
        head_sha: Some("head-sha".to_string()),
        diff_fingerprint: "fingerprint".to_string(),
        working_directory: PathBuf::from("/tmp/worktree"),
        source_pull_request_number: Some(483),
        review_packet: AgentWorkspaceReviewPacket::default(),
    };
    let goal_context = AgentWorkspaceReviewGoalContext {
        user_request_excerpts: vec!["Respect the approved plan.".to_string()],
        ..AgentWorkspaceReviewGoalContext::default()
    };
    let message = build_review_request_message(&workspace, &selected, &goal_context);
    assert!(message.contains("Create or refresh the Review"));
    assert!(message.contains("- Scope: selected_source"));
    assert!(message.contains("- Source pull request: #483"));
    assert!(message.contains("- Review packet: 0 files changed"));
    assert!(message.contains("<workspace_goal_context>"));
    assert!(message.contains("Goal Wins"));
    assert!(message.contains("Respect the approved plan."));
    assert!(message.contains("target.review_packet"));
    assert!(message.contains("list_workspace_review_files"));
    assert!(message.contains("get_workspace_review_diff_page"));
    assert!(message.contains("Do not run shell commands, tests, linters, or validation suites."));
    assert!(message.contains(&workspace.conversation_id.as_str()));
    assert_eq!(
        review_started_summary(&selected),
        "Reviewing selected PR #483 against main."
    );
    assert_eq!(
        workspace_review_conversation_title(&selected),
        "Review PR #483"
    );

    let mut branch = selected.clone();
    branch.source_pull_request_number = None;
    assert_eq!(
        workspace_review_conversation_title(&branch),
        "Review feature/review"
    );
    assert_eq!(
        review_started_summary(&branch),
        "Reviewing selected source branch feature/review against main."
    );

    let mut workspace_delta = selected;
    workspace_delta.scope = AgentWorkspaceReviewTargetScope::WorkspaceDelta;
    assert_eq!(
        workspace_review_conversation_title(&workspace_delta),
        "Review workspace changes"
    );
    assert_eq!(
        review_started_summary(&workspace_delta),
        "Reviewing current workspace changes."
    );
}

#[test]
fn selected_source_review_packet_includes_hunk_anchors() {
    let diff = [
        "diff --git a/src/lib.rs b/src/lib.rs",
        "index 1111111..2222222 100644",
        "--- a/src/lib.rs",
        "+++ b/src/lib.rs",
        "@@ -1,2 +1,3 @@",
        " fn main() {",
        "-    old();",
        "+    new();",
        "+    more();",
        " }",
    ]
    .join("\n");

    let packet = build_selected_source_review_packet(&diff);

    assert_eq!(packet.hunk_anchors.len(), 1);
    let anchor = &packet.hunk_anchors[0];
    assert_eq!(anchor.path, "src/lib.rs");
    assert_eq!(anchor.source, "selected_source");
    assert_eq!(anchor.hunk_header, "@@ -1,2 +1,3 @@");
    assert_eq!(anchor.old_start, 1);
    assert_eq!(anchor.old_lines, 2);
    assert_eq!(anchor.new_start, 1);
    assert_eq!(anchor.new_lines, 3);
}
