use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
use ralphx_lib::application::interactive_notification_producer::question_notification_key;
use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata, InteractiveProcessRegistry,
};
use ralphx_lib::application::{
    AppState, PendingQuestionInfo, QuestionAnswer, QuestionOption, QuestionState,
};
use ralphx_lib::commands::question_commands::{
    resolve_user_question, ResolveQuestionArgs, ResolveQuestionResponse,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, IdeationAnalysisBaseRefKind,
    IdeationSessionFlow, NewNotification, NotificationCategory, NotificationSeverity,
    NotificationTarget, Project,
};
use ralphx_lib::domain::repositories::{QuestionRepository, QueuedMessageRepository};
use ralphx_lib::domain::services::{
    AttachProcessResult, MemoryRunningAgentRegistry, QueueKey, QueuedMessage, RunningAgentInfo,
    RunningAgentKey, RunningAgentRegistry, TryRegisterError,
};
use ralphx_lib::error::{AppError, AppResult};
use ralphx_lib::infrastructure::memory::{MemoryQuestionRepository, MemoryQueuedMessageRepository};
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;
use tokio_util::sync::CancellationToken;

#[test]
fn test_resolve_question_args_deserialize() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": ["opt1", "opt2"], "customResponse": "Custom answer"}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.selected_options, vec!["opt1", "opt2"]);
    assert_eq!(args.custom_response, Some("Custom answer".to_string()));
    assert!(!args.skipped);
}

#[test]
fn test_resolve_question_args_without_custom_response() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": ["opt1"]}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.selected_options, vec!["opt1"]);
    assert!(args.custom_response.is_none());
    assert!(!args.skipped);
}

#[test]
fn test_resolve_question_args_with_skipped() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": [], "skipped": true}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert!(args.selected_options.is_empty());
    assert!(args.custom_response.is_none());
    assert!(args.skipped);
}

#[test]
fn test_resolve_question_response_serialize() {
    let response = ResolveQuestionResponse {
        success: true,
        message: Some("Resolved".to_string()),
        delivered_to_waiting_agent: true,
        plan_mode_proposal_handled: false,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"message\":\"Resolved\""));
    assert!(json.contains("\"deliveredToWaitingAgent\":true"));
    assert!(json.contains("\"planModeProposalHandled\":false"));
}

/// Verify that resolve() returns (true, Some(session_id)) for a known question,
/// which is the condition that gates event emission in resolve_user_question.
#[tokio::test]
async fn test_resolve_returns_true_with_session_id_when_question_exists() {
    let state = QuestionState::new();
    state
        .register(
            "req-abc".to_string(),
            "session-xyz".to_string(),
            "Which option?".to_string(),
            None,
            vec![QuestionOption {
                value: "a".to_string(),
                label: "Option A".to_string(),
                description: None,
            }],
            false,
        )
        .await;

    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string()],
        text: None,
        skipped: false,
    };
    let result = state.resolve("req-abc", answer).await;

    // emit path should be taken: resolved == true and session_id.is_some()
    assert!(
        result.resolved,
        "resolve should return true for a known request_id"
    );
    assert_eq!(
        result.session_id,
        Some("session-xyz".to_string()),
        "session_id should match the registered session"
    );
    assert!(result.delivered_to_waiting_agent);
}

/// Verify that resolve() returns (false, None) for an unknown question,
/// which means the event emission path is NOT taken.
#[tokio::test]
async fn test_resolve_returns_false_when_question_not_found() {
    let state = QuestionState::new();

    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string()],
        text: None,
        skipped: false,
    };
    let result = state.resolve("nonexistent-req", answer).await;

    // emit path should NOT be taken: resolved == false
    assert!(
        !result.resolved,
        "resolve should return false for an unknown request_id"
    );
    assert!(
        result.session_id.is_none(),
        "session_id should be None when not resolved"
    );
    assert!(!result.delivered_to_waiting_agent);
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

fn build_question_command_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(Arc::new(ExecutionState::new()))
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

async fn setup_accepted_plan_mode_proposal_question(
    state: &AppState,
    request_id: &str,
) -> (
    tempfile::TempDir,
    ralphx_lib::domain::entities::ChatConversationId,
    tokio::sync::watch::Receiver<Option<QuestionAnswer>>,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);

    let mut project = Project::new(
        "Plan Proposal".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Edit);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let conversation_id = conversation.id;
    let conversation_id_string = conversation_id.as_str();

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
    .expect("edit workspace should be prepared");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let receiver = state
        .question_state
        .register_with_metadata(
            request_id.to_string(),
            conversation_id_string,
            "Switch to Plan mode?".to_string(),
            None,
            vec![QuestionOption {
                value: "switch_to_plan".to_string(),
                label: "Switch to Plan".to_string(),
                description: None,
            }],
            false,
            true,
            None,
            None,
            Some(json!({
                "kind": "plan_mode_proposal",
                "conversation_id": conversation_id.as_str(),
                "reason": "Draft the implementation plan first"
            })),
        )
        .await;

    (temp, conversation_id, receiver)
}

#[tokio::test]
async fn resolving_question_marks_its_durable_notification_read() {
    let state = AppState::new_test();
    state
        .question_state
        .register(
            "question-notification".to_string(),
            "session-question".to_string(),
            "Continue?".to_string(),
            None,
            vec![QuestionOption {
                value: "yes".to_string(),
                label: "Yes".to_string(),
                description: None,
            }],
            false,
        )
        .await;
    state
        .notification_service()
        .record(NewNotification {
            project_id: None,
            category: NotificationCategory::AgentQuestion,
            severity: NotificationSeverity::ActionRequired,
            title: "Question".to_string(),
            body: None,
            target: NotificationTarget::none(),
            dedupe_key: Some(question_notification_key("question-notification")),
        })
        .await;

    let app = build_question_command_app(state);
    let response = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "question-notification".to_string(),
            selected_options: vec!["yes".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await
    .expect("question should resolve");

    assert!(response.success);
    let notifications = app
        .state::<AppState>()
        .notification_repo
        .list(None, None, 10)
        .await
        .expect("notifications should load")
        .notifications;
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0].read_at.is_some());
}

#[tokio::test]
async fn accepted_plan_mode_proposal_links_planning_session_before_hidden_continuation() {
    let state = AppState::new_test();
    let (_temp, conversation_id, _receiver) =
        setup_accepted_plan_mode_proposal_question(&state, "req-plan").await;
    let conversation_id_string = conversation_id.as_str();

    let app = build_question_command_app(state);
    let response = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await
    .expect("question should resolve");

    assert!(response.success);
    assert!(response.delivered_to_waiting_agent);
    assert!(response.plan_mode_proposal_handled);

    let state = app.state::<AppState>();
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup should succeed")
        .expect("conversation should exist");
    assert_eq!(
        conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Plan)
    );

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    let planning_session_id = workspace
        .linked_ideation_session_id
        .clone()
        .expect("plan workspace should link to a planning session");
    assert!(
        workspace.linked_plan_branch_id.is_none(),
        "Plan-mode handoff should start with a planning session, not a plan branch"
    );

    let session = state
        .ideation_session_repo
        .get_by_id(&planning_session_id)
        .await
        .expect("planning session lookup should succeed")
        .expect("planning session should exist");
    assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
    assert_eq!(
        session.source_context_type.as_deref(),
        Some("agent_conversation")
    );
    assert_eq!(
        session.source_context_id.as_deref(),
        Some(conversation_id_string.as_str())
    );
    assert_eq!(session.spawn_reason.as_deref(), Some("agent_plan_mode"));

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Project, conversation_id_string.as_str());
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].id, "plan-mode-handoff:req-plan");
    assert!(queued[0]
        .metadata_override
        .as_deref()
        .expect("queued continuation should carry metadata")
        .contains("\"source\":\"accepted_plan_mode_proposal\""));
    assert!(queued[0]
        .metadata_override
        .as_deref()
        .expect("queued continuation should carry metadata")
        .contains("\"resume_in_place\":true"));
    assert!(queued[0]
        .metadata_override
        .as_deref()
        .expect("queued continuation should carry metadata")
        .contains("\"source_request_id\":\"req-plan\""));
    assert!(queued[0]
        .metadata_override
        .as_deref()
        .expect("queued continuation should carry metadata")
        .contains("\"required_workspace_mode\":\"plan\""));

    let durable = state
        .queued_message_repo
        .list(&QueueKey::new(
            ChatContextType::Project,
            conversation_id_string.as_str(),
        ))
        .await
        .expect("durable queue should load");
    assert_eq!(durable.len(), 1, "handoff must be restart recoverable");
    assert_eq!(durable[0].id, "plan-mode-handoff:req-plan");

    let duplicate = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await;
    assert!(
        duplicate.is_err(),
        "a consumed question claim must not be accepted twice"
    );

    let durable_after_duplicate = state
        .queued_message_repo
        .list(&QueueKey::new(
            ChatContextType::Project,
            conversation_id_string.as_str(),
        ))
        .await
        .expect("durable queue should still load");
    assert!(
        durable_after_duplicate.len() <= 1,
        "duplicate acceptance must not create a second continuation row"
    );
}

#[tokio::test]
async fn accepted_plan_mode_proposal_reservation_blocks_competing_launch_during_staging_and_releases_before_kick(
) {
    struct CompetitorDuringStagingQueueRepo {
        inner: Arc<MemoryQueuedMessageRepository>,
        running_agent_registry: Arc<MemoryRunningAgentRegistry>,
        interactive_process_registry: Arc<InteractiveProcessRegistry>,
        competitor_claimed: AtomicBool,
        competitor_ipr_registered: AtomicBool,
        competitor_child: tokio::sync::Mutex<Option<tokio::process::Child>>,
    }

    #[async_trait::async_trait]
    impl QueuedMessageRepository for CompetitorDuringStagingQueueRepo {
        async fn enqueue_back(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
            if message.id.starts_with("plan-mode-handoff:") {
                let running_key =
                    RunningAgentKey::new(key.context_type.to_string(), key.context_id.clone());
                if self
                    .running_agent_registry
                    .try_register(
                        running_key.clone(),
                        key.context_id.clone(),
                        "competing-during-staging".to_string(),
                    )
                    .await
                    .is_ok()
                {
                    self.competitor_claimed.store(true, Ordering::SeqCst);
                    let mut child = tokio::process::Command::new("cat")
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .expect("competing runtime stdin fixture should start");
                    let stdin = child
                        .stdin
                        .take()
                        .expect("competing runtime should expose stdin");
                    self.interactive_process_registry
                        .register_with_metadata(
                            InteractiveProcessKey::new(
                                key.context_type.to_string(),
                                &key.context_id,
                            ),
                            stdin,
                            InteractiveProcessMetadata {
                                agent_run_id: Some("competing-during-staging".to_string()),
                                ..Default::default()
                            },
                        )
                        .await;
                    self.competitor_ipr_registered.store(true, Ordering::SeqCst);
                    *self.competitor_child.lock().await = Some(child);
                }
            }
            self.inner.enqueue_back(key, message).await
        }

        async fn enqueue_front(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
            self.inner.enqueue_front(key, message).await
        }

        async fn list(&self, key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
            self.inner.list(key).await
        }

        async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
            self.inner.list_keys().await
        }

        async fn delete(&self, key: &QueueKey, message_id: &str) -> AppResult<bool> {
            self.inner.delete(key, message_id).await
        }

        async fn delete_by_id(&self, message_id: &str) -> AppResult<bool> {
            self.inner.delete_by_id(message_id).await
        }

        async fn clear(&self, key: &QueueKey) -> AppResult<()> {
            self.inner.clear(key).await
        }

        async fn pop_front(&self, key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
            self.inner.pop_front(key).await
        }

        async fn remove_stale(
            &self,
            key: &QueueKey,
            threshold_secs: u64,
        ) -> AppResult<Vec<QueuedMessage>> {
            self.inner.remove_stale(key, threshold_secs).await
        }
    }

    let mut state = AppState::new_test();
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());
    let durable_queue = Arc::new(MemoryQueuedMessageRepository::new());
    let competing_queue_repo = Arc::new(CompetitorDuringStagingQueueRepo {
        inner: Arc::clone(&durable_queue),
        running_agent_registry: Arc::clone(&running_agent_registry),
        interactive_process_registry: Arc::clone(&state.interactive_process_registry),
        competitor_claimed: AtomicBool::new(false),
        competitor_ipr_registered: AtomicBool::new(false),
        competitor_child: tokio::sync::Mutex::new(None),
    });
    state.running_agent_registry = running_agent_registry.clone();
    state.queued_message_repo = competing_queue_repo.clone();
    let (_temp, conversation_id, receiver) =
        setup_accepted_plan_mode_proposal_question(&state, "req-plan-reservation-wins").await;
    let app = build_question_command_app(state);

    let response = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan-reservation-wins".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await
    .expect("the request-owned reservation should permit the answer commit");

    assert!(response.success);
    assert!(receiver.borrow().is_some(), "the answer must commit");
    assert!(
        !competing_queue_repo
            .competitor_claimed
            .load(Ordering::SeqCst),
        "a competing launch must not claim the running-agent slot during durable staging"
    );
    assert!(
        !competing_queue_repo
            .competitor_ipr_registered
            .load(Ordering::SeqCst),
        "a launch that cannot claim the slot must not register an interactive process"
    );

    let running_key = RunningAgentKey::new("project", conversation_id.as_str());
    assert!(
        running_agent_registry.get(&running_key).await.is_none(),
        "the request-owned PID-0 reservation must release before the post-commit kick"
    );
    assert!(
        durable_queue
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id.as_str(),
            ))
            .await
            .expect("durable queue should remain readable")
            .iter()
            .any(|message| message.id == "plan-mode-handoff:req-plan-reservation-wins"),
        "the kick must rely on the durable continuation rather than the released reservation"
    );

    let child = { competing_queue_repo.competitor_child.lock().await.take() };
    if let Some(mut child) = child {
        child
            .kill()
            .await
            .expect("competing runtime fixture should stop cleanly");
        let _ = child.wait().await;
    }
}

#[tokio::test]
async fn accepted_plan_mode_proposal_is_unhandled_when_post_commit_handoff_kick_cannot_verify_row()
{
    struct PostCommitKickVerificationFailureRepo {
        inner: Arc<MemoryQueuedMessageRepository>,
        fail_next_delete: AtomicBool,
        fail_next_list: AtomicBool,
    }

    #[async_trait::async_trait]
    impl QueuedMessageRepository for PostCommitKickVerificationFailureRepo {
        async fn enqueue_back(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
            self.inner.enqueue_back(key, message).await
        }

        async fn enqueue_front(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
            self.inner.enqueue_front(key, message).await
        }

        async fn list(&self, key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
            if self.fail_next_list.swap(false, Ordering::SeqCst) {
                return Err(AppError::Database(
                    "injected post-commit handoff verification failure".to_string(),
                ));
            }
            self.inner.list(key).await
        }

        async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
            self.inner.list_keys().await
        }

        async fn delete(&self, key: &QueueKey, message_id: &str) -> AppResult<bool> {
            if self.fail_next_delete.swap(false, Ordering::SeqCst) {
                return Err(AppError::Database(
                    "injected post-commit handoff kick failure".to_string(),
                ));
            }
            self.inner.delete(key, message_id).await
        }

        async fn delete_by_id(&self, message_id: &str) -> AppResult<bool> {
            self.inner.delete_by_id(message_id).await
        }

        async fn clear(&self, key: &QueueKey) -> AppResult<()> {
            self.inner.clear(key).await
        }

        async fn pop_front(&self, key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
            self.inner.pop_front(key).await
        }

        async fn remove_stale(
            &self,
            key: &QueueKey,
            threshold_secs: u64,
        ) -> AppResult<Vec<QueuedMessage>> {
            self.inner.remove_stale(key, threshold_secs).await
        }
    }

    let mut state = AppState::new_test();
    let durable_queue = Arc::new(MemoryQueuedMessageRepository::new());
    state.queued_message_repo = Arc::new(PostCommitKickVerificationFailureRepo {
        inner: Arc::clone(&durable_queue),
        fail_next_delete: AtomicBool::new(true),
        fail_next_list: AtomicBool::new(true),
    });
    let (_temp, conversation_id, receiver) =
        setup_accepted_plan_mode_proposal_question(&state, "req-plan-post-commit-kick-failure")
            .await;
    let app = build_question_command_app(state);

    let response = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan-post-commit-kick-failure".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await
    .expect("the question answer should still commit");

    assert!(response.success);
    assert!(response.delivered_to_waiting_agent);
    assert!(
        !response.plan_mode_proposal_handled,
        "an unreadable recovery row must not suppress the frontend fallback"
    );
    assert!(
        receiver.borrow().is_some(),
        "the answer commit must complete"
    );

    let queue_key = QueueKey::new(ChatContextType::Project, conversation_id.as_str());
    let durable = durable_queue
        .list(&queue_key)
        .await
        .expect("the independently staged durable row must remain recoverable");
    assert_eq!(durable.len(), 1);
    assert_eq!(
        durable[0].id,
        "plan-mode-handoff:req-plan-post-commit-kick-failure"
    );

    let duplicate = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan-post-commit-kick-failure".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await;
    assert!(
        duplicate.is_err(),
        "a committed answer cannot stage a second handoff"
    );
    assert_eq!(
        durable_queue
            .list(&queue_key)
            .await
            .expect("the durable row must still load")
            .len(),
        1,
        "recovery has one exact continuation to process"
    );
}

#[tokio::test]
async fn accepted_plan_mode_proposal_is_unhandled_when_no_owner_reservation_release_is_unverified()
{
    struct ReservationRetainingRegistry {
        inner: Arc<MemoryRunningAgentRegistry>,
        release_attempted: AtomicBool,
    }

    #[async_trait::async_trait]
    impl RunningAgentRegistry for ReservationRetainingRegistry {
        async fn register(
            &self,
            key: RunningAgentKey,
            pid: u32,
            conversation_id: String,
            agent_run_id: String,
            worktree_path: Option<String>,
            cancellation_token: Option<CancellationToken>,
        ) {
            self.inner
                .register(
                    key,
                    pid,
                    conversation_id,
                    agent_run_id,
                    worktree_path,
                    cancellation_token,
                )
                .await;
        }

        async fn unregister(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Option<RunningAgentInfo> {
            if agent_run_id.starts_with("plan-mode-handoff-reservation:") {
                self.release_attempted.store(true, Ordering::SeqCst);
                return None;
            }
            self.inner.unregister(key, agent_run_id).await
        }

        async fn get(&self, key: &RunningAgentKey) -> Option<RunningAgentInfo> {
            self.inner.get(key).await
        }

        async fn is_running(&self, key: &RunningAgentKey) -> bool {
            self.inner.is_running(key).await
        }

        async fn stop(&self, key: &RunningAgentKey) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.stop(key).await
        }

        async fn stop_if_owned(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.stop_if_owned(key, agent_run_id).await
        }

        async fn quiesce_if_owned(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.quiesce_if_owned(key, agent_run_id).await
        }

        async fn list_all(&self) -> Vec<(RunningAgentKey, RunningAgentInfo)> {
            self.inner.list_all().await
        }

        async fn list_by_context_type(
            &self,
            context_type: &str,
        ) -> Result<Vec<(RunningAgentKey, RunningAgentInfo)>, String> {
            self.inner.list_by_context_type(context_type).await
        }

        async fn stop_all(&self) -> Vec<RunningAgentKey> {
            self.inner.stop_all().await
        }

        async fn stop_all_started_before(
            &self,
            cutoff: chrono::DateTime<chrono::Utc>,
        ) -> Vec<RunningAgentKey> {
            self.inner.stop_all_started_before(cutoff).await
        }

        async fn update_heartbeat(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
            at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, String> {
            self.inner.update_heartbeat(key, agent_run_id, at).await
        }

        async fn try_register(
            &self,
            key: RunningAgentKey,
            conversation_id: String,
            agent_run_id: String,
        ) -> Result<(), TryRegisterError> {
            self.inner
                .try_register(key, conversation_id, agent_run_id)
                .await
        }

        async fn renew_reservation(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
            at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, String> {
            self.inner.renew_reservation(key, agent_run_id, at).await
        }

        async fn attach_process(
            &self,
            key: &RunningAgentKey,
            expected_agent_run_id: &str,
            pid: u32,
            worktree_path: Option<String>,
            cancellation_token: Option<CancellationToken>,
            model: Option<String>,
        ) -> Result<AttachProcessResult, String> {
            self.inner
                .attach_process(
                    key,
                    expected_agent_run_id,
                    pid,
                    worktree_path,
                    cancellation_token,
                    model,
                )
                .await
        }

        async fn cleanup_stale_entry(
            &self,
            key: &RunningAgentKey,
            expected_agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner
                .cleanup_stale_entry(key, expected_agent_run_id)
                .await
        }
    }

    struct DeleteCountingQueueRepo {
        inner: Arc<MemoryQueuedMessageRepository>,
        delete_calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl QueuedMessageRepository for DeleteCountingQueueRepo {
        async fn enqueue_back(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
            self.inner.enqueue_back(key, message).await
        }

        async fn enqueue_front(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
            self.inner.enqueue_front(key, message).await
        }

        async fn list(&self, key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
            self.inner.list(key).await
        }

        async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
            self.inner.list_keys().await
        }

        async fn delete(&self, key: &QueueKey, message_id: &str) -> AppResult<bool> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.delete(key, message_id).await
        }

        async fn delete_by_id(&self, message_id: &str) -> AppResult<bool> {
            self.inner.delete_by_id(message_id).await
        }

        async fn clear(&self, key: &QueueKey) -> AppResult<()> {
            self.inner.clear(key).await
        }

        async fn pop_front(&self, key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
            self.inner.pop_front(key).await
        }

        async fn remove_stale(
            &self,
            key: &QueueKey,
            threshold_secs: u64,
        ) -> AppResult<Vec<QueuedMessage>> {
            self.inner.remove_stale(key, threshold_secs).await
        }
    }

    let mut state = AppState::new_test();
    let inner_registry = Arc::new(MemoryRunningAgentRegistry::new());
    let retaining_registry = Arc::new(ReservationRetainingRegistry {
        inner: Arc::clone(&inner_registry),
        release_attempted: AtomicBool::new(false),
    });
    let durable_queue = Arc::new(MemoryQueuedMessageRepository::new());
    let counting_queue = Arc::new(DeleteCountingQueueRepo {
        inner: Arc::clone(&durable_queue),
        delete_calls: AtomicUsize::new(0),
    });
    state.running_agent_registry = retaining_registry.clone();
    state.queued_message_repo = counting_queue.clone();
    let (_temp, conversation_id, receiver) =
        setup_accepted_plan_mode_proposal_question(&state, "req-plan-release-unverified").await;
    let app = build_question_command_app(state);

    let response = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan-release-unverified".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await
    .expect("the durable answer commit should succeed");

    assert!(response.success);
    assert!(response.delivered_to_waiting_agent);
    assert!(
        !response.plan_mode_proposal_handled,
        "an unverified release must not report the durable handoff as handled"
    );
    assert!(
        receiver.borrow().is_some(),
        "the answer commit must complete even when launch recovery is deferred"
    );
    assert!(
        retaining_registry.release_attempted.load(Ordering::SeqCst),
        "the command must attempt exact-owner reservation release"
    );
    assert_eq!(
        counting_queue.delete_calls.load(Ordering::SeqCst),
        0,
        "an unverified release must not kick and consume the durable continuation"
    );

    let running_key = RunningAgentKey::new("project", conversation_id.as_str());
    let reservation = inner_registry
        .get(&running_key)
        .await
        .expect("the injected failed release must retain the exact reservation");
    assert_eq!(
        reservation.agent_run_id,
        "plan-mode-handoff-reservation:req-plan-release-unverified"
    );
    assert!(
        durable_queue
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id.as_str(),
            ))
            .await
            .expect("the durable continuation should remain readable")
            .iter()
            .any(|message| message.id == "plan-mode-handoff:req-plan-release-unverified"),
        "the unreleased reservation leaves one recoverable continuation"
    );
}

#[tokio::test]
async fn accepted_plan_mode_proposal_commit_failure_compensates_staged_handoff() {
    struct FailingResolveRepo(MemoryQuestionRepository);

    #[async_trait::async_trait]
    impl QuestionRepository for FailingResolveRepo {
        async fn create_pending(&self, info: &PendingQuestionInfo) -> AppResult<()> {
            self.0.create_pending(info).await
        }

        async fn resolve(&self, _request_id: &str, _answer: &QuestionAnswer) -> AppResult<bool> {
            Err(AppError::Database("durable write failed".to_string()))
        }

        async fn get_pending(&self) -> AppResult<Vec<PendingQuestionInfo>> {
            self.0.get_pending().await
        }

        async fn get_by_request_id(
            &self,
            request_id: &str,
        ) -> AppResult<Option<PendingQuestionInfo>> {
            self.0.get_by_request_id(request_id).await
        }

        async fn expire_all_pending(&self) -> AppResult<u64> {
            self.0.expire_all_pending().await
        }

        async fn expire_by_request_id(&self, request_id: &str) -> AppResult<()> {
            self.0.expire_by_request_id(request_id).await
        }

        async fn remove(&self, request_id: &str) -> AppResult<bool> {
            self.0.remove(request_id).await
        }

        async fn get_resolved_answer(&self, request_id: &str) -> AppResult<Option<QuestionAnswer>> {
            self.0.get_resolved_answer(request_id).await
        }
    }

    let mut state = AppState::new_test();
    state.question_state = Arc::new(QuestionState::with_repo(Arc::new(FailingResolveRepo(
        MemoryQuestionRepository::new(),
    ))));
    let (_temp, conversation_id, receiver) =
        setup_accepted_plan_mode_proposal_question(&state, "req-plan-commit-failure").await;
    let conversation_id_string = conversation_id.as_str();
    let app = build_question_command_app(state);

    let result = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan-commit-failure".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await;

    assert!(
        result.is_err(),
        "a failed durable answer commit must not report a handled proposal"
    );
    assert!(
        receiver.borrow().is_none(),
        "the live waiter must not receive an answer before durable resolution"
    );

    let state = app.state::<AppState>();
    assert!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, conversation_id_string.as_str())
            .is_empty(),
        "compensation must remove the in-memory handoff row"
    );
    assert!(
        state
            .queued_message_repo
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id_string.as_str(),
            ))
            .await
            .expect("durable handoff should load")
            .is_empty(),
        "compensation must remove the durable handoff row"
    );
    assert!(
        state
            .running_agent_registry
            .get(&RunningAgentKey::new(
                "project",
                conversation_id_string.as_str()
            ))
            .await
            .is_none(),
        "a failed answer commit must release the no-owner reservation"
    );

    let retry_claim = state
        .question_state
        .claim_pending("req-plan-commit-failure")
        .await
        .expect("failed durable commit should leave the question claimable")
        .expect("failed durable commit should retain the question");
    assert!(state.question_state.release_claim(retry_claim).await);
}

#[tokio::test]
async fn accepted_plan_mode_proposal_fails_closed_when_running_registry_read_fails() {
    struct FailingRunningRegistry {
        inner: Arc<MemoryRunningAgentRegistry>,
    }

    #[async_trait::async_trait]
    impl RunningAgentRegistry for FailingRunningRegistry {
        async fn register(
            &self,
            key: RunningAgentKey,
            pid: u32,
            conversation_id: String,
            agent_run_id: String,
            worktree_path: Option<String>,
            cancellation_token: Option<CancellationToken>,
        ) {
            self.inner
                .register(
                    key,
                    pid,
                    conversation_id,
                    agent_run_id,
                    worktree_path,
                    cancellation_token,
                )
                .await;
        }

        async fn unregister(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Option<RunningAgentInfo> {
            self.inner.unregister(key, agent_run_id).await
        }

        async fn get(&self, key: &RunningAgentKey) -> Option<RunningAgentInfo> {
            self.inner.get(key).await
        }

        async fn is_running(&self, key: &RunningAgentKey) -> bool {
            self.inner.is_running(key).await
        }

        async fn stop(&self, key: &RunningAgentKey) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.stop(key).await
        }

        async fn stop_if_owned(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.stop_if_owned(key, agent_run_id).await
        }

        async fn quiesce_if_owned(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.quiesce_if_owned(key, agent_run_id).await
        }

        async fn list_all(&self) -> Vec<(RunningAgentKey, RunningAgentInfo)> {
            self.inner.list_all().await
        }

        async fn list_by_context_type(
            &self,
            _context_type: &str,
        ) -> Result<Vec<(RunningAgentKey, RunningAgentInfo)>, String> {
            Err("injected running registry read failure".to_string())
        }

        async fn stop_all(&self) -> Vec<RunningAgentKey> {
            self.inner.stop_all().await
        }

        async fn stop_all_started_before(
            &self,
            cutoff: chrono::DateTime<chrono::Utc>,
        ) -> Vec<RunningAgentKey> {
            self.inner.stop_all_started_before(cutoff).await
        }

        async fn update_heartbeat(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
            at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, String> {
            self.inner.update_heartbeat(key, agent_run_id, at).await
        }

        async fn try_register(
            &self,
            key: RunningAgentKey,
            conversation_id: String,
            agent_run_id: String,
        ) -> Result<(), TryRegisterError> {
            self.inner
                .try_register(key, conversation_id, agent_run_id)
                .await
        }

        async fn renew_reservation(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
            at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, String> {
            self.inner.renew_reservation(key, agent_run_id, at).await
        }

        async fn attach_process(
            &self,
            key: &RunningAgentKey,
            expected_agent_run_id: &str,
            pid: u32,
            worktree_path: Option<String>,
            cancellation_token: Option<CancellationToken>,
            model: Option<String>,
        ) -> Result<AttachProcessResult, String> {
            self.inner
                .attach_process(
                    key,
                    expected_agent_run_id,
                    pid,
                    worktree_path,
                    cancellation_token,
                    model,
                )
                .await
        }

        async fn cleanup_stale_entry(
            &self,
            key: &RunningAgentKey,
            expected_agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner
                .cleanup_stale_entry(key, expected_agent_run_id)
                .await
        }
    }

    let mut state = AppState::new_test();
    let (_temp, conversation_id, receiver) =
        setup_accepted_plan_mode_proposal_question(&state, "req-plan-registry-read-error").await;
    state.running_agent_registry = Arc::new(FailingRunningRegistry {
        inner: Arc::new(MemoryRunningAgentRegistry::new()),
    });
    let app = build_question_command_app(state);

    let result = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan-registry-read-error".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await;

    let error = result.expect_err("registry read failure must reject the accepted proposal");
    assert!(error.contains("stable runtime-handoff ownership"));
    assert!(
        receiver.borrow().is_none(),
        "an uncommitted answer must not reach the waiting agent"
    );
    let state = app.state::<AppState>();
    assert!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation_id.as_str())
            .is_empty(),
        "capture failure must not stage an in-memory continuation"
    );
    assert!(
        state
            .queued_message_repo
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id.as_str(),
            ))
            .await
            .expect("durable queue should be readable")
            .is_empty(),
        "capture failure must not stage a durable continuation"
    );
    let retry_claim = state
        .question_state
        .claim_pending("req-plan-registry-read-error")
        .await
        .expect("failed capture should keep the question reclaimable")
        .expect("question should remain pending after failed capture");
    assert!(state.question_state.release_claim(retry_claim).await);
}

#[tokio::test]
async fn accepted_plan_mode_proposal_rejects_when_competing_launch_claims_before_no_owner_reservation(
) {
    struct OwnerInjectingRunningRegistry {
        inner: Arc<MemoryRunningAgentRegistry>,
        injected: AtomicBool,
    }

    #[async_trait::async_trait]
    impl RunningAgentRegistry for OwnerInjectingRunningRegistry {
        async fn register(
            &self,
            key: RunningAgentKey,
            pid: u32,
            conversation_id: String,
            agent_run_id: String,
            worktree_path: Option<String>,
            cancellation_token: Option<CancellationToken>,
        ) {
            self.inner
                .register(
                    key,
                    pid,
                    conversation_id,
                    agent_run_id,
                    worktree_path,
                    cancellation_token,
                )
                .await;
        }

        async fn unregister(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Option<RunningAgentInfo> {
            self.inner.unregister(key, agent_run_id).await
        }

        async fn get(&self, key: &RunningAgentKey) -> Option<RunningAgentInfo> {
            self.inner.get(key).await
        }

        async fn is_running(&self, key: &RunningAgentKey) -> bool {
            self.inner.is_running(key).await
        }

        async fn stop(&self, key: &RunningAgentKey) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.stop(key).await
        }

        async fn stop_if_owned(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.stop_if_owned(key, agent_run_id).await
        }

        async fn quiesce_if_owned(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner.quiesce_if_owned(key, agent_run_id).await
        }

        async fn list_all(&self) -> Vec<(RunningAgentKey, RunningAgentInfo)> {
            self.inner.list_all().await
        }

        async fn list_by_context_type(
            &self,
            context_type: &str,
        ) -> Result<Vec<(RunningAgentKey, RunningAgentInfo)>, String> {
            self.inner.list_by_context_type(context_type).await
        }

        async fn stop_all(&self) -> Vec<RunningAgentKey> {
            self.inner.stop_all().await
        }

        async fn stop_all_started_before(
            &self,
            cutoff: chrono::DateTime<chrono::Utc>,
        ) -> Vec<RunningAgentKey> {
            self.inner.stop_all_started_before(cutoff).await
        }

        async fn update_heartbeat(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
            at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, String> {
            self.inner.update_heartbeat(key, agent_run_id, at).await
        }

        async fn try_register(
            &self,
            key: RunningAgentKey,
            conversation_id: String,
            agent_run_id: String,
        ) -> Result<(), TryRegisterError> {
            if agent_run_id.starts_with("plan-mode-handoff-reservation:")
                && self
                    .injected
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                self.inner
                    .try_register(
                        key.clone(),
                        key.context_id.clone(),
                        "competing-launch".to_string(),
                    )
                    .await
                    .expect("the competing launch should win the empty slot");
            }
            self.inner
                .try_register(key, conversation_id, agent_run_id)
                .await
        }

        async fn renew_reservation(
            &self,
            key: &RunningAgentKey,
            agent_run_id: &str,
            at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, String> {
            self.inner.renew_reservation(key, agent_run_id, at).await
        }

        async fn attach_process(
            &self,
            key: &RunningAgentKey,
            expected_agent_run_id: &str,
            pid: u32,
            worktree_path: Option<String>,
            cancellation_token: Option<CancellationToken>,
            model: Option<String>,
        ) -> Result<AttachProcessResult, String> {
            self.inner
                .attach_process(
                    key,
                    expected_agent_run_id,
                    pid,
                    worktree_path,
                    cancellation_token,
                    model,
                )
                .await
        }

        async fn cleanup_stale_entry(
            &self,
            key: &RunningAgentKey,
            expected_agent_run_id: &str,
        ) -> Result<Option<RunningAgentInfo>, String> {
            self.inner
                .cleanup_stale_entry(key, expected_agent_run_id)
                .await
        }
    }

    let mut state = AppState::new_test();
    let (_temp, conversation_id, receiver) =
        setup_accepted_plan_mode_proposal_question(&state, "req-plan-stale-no-owner").await;
    let inner = Arc::new(MemoryRunningAgentRegistry::new());
    let injecting_registry = Arc::new(OwnerInjectingRunningRegistry {
        inner: Arc::clone(&inner),
        injected: AtomicBool::new(false),
    });
    state.running_agent_registry = injecting_registry.clone();
    let app = build_question_command_app(state);

    let result = resolve_user_question(
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
        ResolveQuestionArgs {
            request_id: "req-plan-stale-no-owner".to_string(),
            selected_options: vec!["switch_to_plan".to_string()],
            custom_response: None,
            skipped: false,
        },
    )
    .await;

    let error = result.expect_err("a competing launch must reject the answer before commit");
    assert!(error.contains("stable runtime-handoff ownership"));
    assert!(
        receiver.borrow().is_none(),
        "an uncommitted answer must not reach the waiting agent"
    );

    let state = app.state::<AppState>();
    let running_key = RunningAgentKey::new("project", conversation_id.as_str());
    let running_owner = inner
        .get(&running_key)
        .await
        .expect("competing launch must remain registered");
    assert_eq!(running_owner.agent_run_id, "competing-launch");
    assert!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation_id.as_str())
            .is_empty(),
        "failure must not stage an in-memory continuation"
    );
    assert!(
        state
            .queued_message_repo
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id.as_str(),
            ))
            .await
            .expect("durable queue should be readable")
            .is_empty(),
        "failure must not stage a durable continuation"
    );
    let retry_claim = state
        .question_state
        .claim_pending("req-plan-stale-no-owner")
        .await
        .expect("competing launch should leave the question reclaimable")
        .expect("question should remain pending after failed revalidation");
    assert!(state.question_state.release_claim(retry_claim).await);
}
