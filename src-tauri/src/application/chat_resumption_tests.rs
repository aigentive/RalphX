use super::*;
use crate::application::AppState;
use crate::domain::entities::{AgentRun, ChatConversation, InternalStatus, Project, Task};

/// Helper to create test state
async fn setup_test_state() -> (Arc<ExecutionState>, AppState) {
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    (execution_state, app_state)
}

/// Helper to build a ChatResumptionRunner from test state
fn build_runner(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> ChatResumptionRunner<tauri::Wry> {
    ChatResumptionRunner::new(
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(execution_state),
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_core(
            Arc::clone(&app_state.chat_message_repo),
            Arc::clone(&app_state.chat_attachment_repo),
            Arc::clone(&app_state.artifact_repo),
            Arc::clone(&app_state.chat_conversation_repo),
            Arc::clone(&app_state.agent_run_repo),
            Arc::clone(&app_state.project_repo),
            Arc::clone(&app_state.task_repo),
            Arc::clone(&app_state.task_dependency_repo),
            Arc::clone(&app_state.ideation_session_repo),
            Arc::clone(&app_state.activity_event_repo),
            Arc::clone(&app_state.message_queue),
            Arc::clone(&app_state.running_agent_registry),
            Arc::clone(&app_state.memory_event_repo),
            Arc::clone(&app_state.project_memory_settings_repo),
        ),
    )
}

#[test]
fn test_context_type_priority_ordering() {
    // TaskExecution should have highest priority (lowest number)
    assert!(
        context_type_priority(ChatContextType::TaskExecution)
            < context_type_priority(ChatContextType::Review)
    );
    assert!(
        context_type_priority(ChatContextType::Review)
            < context_type_priority(ChatContextType::Task)
    );
    assert!(
        context_type_priority(ChatContextType::Task)
            < context_type_priority(ChatContextType::Ideation)
    );
    assert!(
        context_type_priority(ChatContextType::Ideation)
            < context_type_priority(ChatContextType::Project)
    );
}

#[test]
fn test_prioritize_resumptions_sorts_correctly() {
    // Create test conversations with different context types
    let create_interrupted = |context_type: ChatContextType| -> InterruptedConversation {
        let mut conv =
            ChatConversation::new_ideation(crate::domain::entities::IdeationSessionId::new());
        // Override context_type for testing (normally set by constructor)
        conv.context_type = context_type;
        conv.context_id = "test-id".to_string();
        conv.claude_session_id = Some("test-session".to_string());

        let run = AgentRun::new(conv.id);

        InterruptedConversation {
            conversation: conv,
            last_run: run,
        }
    };

    let conversations = vec![
        create_interrupted(ChatContextType::Project), // Lowest priority
        create_interrupted(ChatContextType::TaskExecution), // Highest priority
        create_interrupted(ChatContextType::Ideation),
        create_interrupted(ChatContextType::Review),
        create_interrupted(ChatContextType::Task),
    ];

    // Use a temporary runner just for the sort function
    let sorted = {
        let mut convs = conversations;
        convs.sort_by_key(|conv| context_type_priority(conv.conversation.context_type));
        convs
    };

    // Verify order: TaskExecution, Review, Task, Ideation, Project
    assert_eq!(
        sorted[0].conversation.context_type,
        ChatContextType::TaskExecution
    );
    assert_eq!(sorted[1].conversation.context_type, ChatContextType::Review);
    assert_eq!(sorted[2].conversation.context_type, ChatContextType::Task);
    assert_eq!(
        sorted[3].conversation.context_type,
        ChatContextType::Ideation
    );
    assert_eq!(
        sorted[4].conversation.context_type,
        ChatContextType::Project
    );
}

#[tokio::test]
async fn test_resumption_skipped_when_paused() {
    let (execution_state, app_state) = setup_test_state().await;

    // Pause execution
    execution_state.pause();

    let runner = build_runner(&app_state, &execution_state);

    // Run should skip because paused - just verify it doesn't panic
    runner.run().await;

    // Verify no conversations were created (nothing resumed)
    // The mock repo returns empty for get_interrupted_conversations, so this is a no-op
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_agent_active_task() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project and task in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Create an interrupted conversation for TaskExecution
    let mut conv = ChatConversation::new_task_execution(task_id.clone());
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Should be handled by task resumption (task is in Executing status)
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        is_handled,
        "TaskExecution with Executing task should be handled by StartupJobRunner"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_non_agent_active_task() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project and task in Ready state (NOT agent-active)
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Create an interrupted conversation for TaskExecution
    let mut conv = ChatConversation::new_task_execution(task_id.clone());
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Should NOT be handled by task resumption (task is in Ready status)
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        !is_handled,
        "TaskExecution with Ready task should NOT be handled by StartupJobRunner"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_ideation() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create an interrupted conversation for Ideation
    let session_id = crate::domain::entities::IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Ideation IS handled by the dedicated recovery loop (Phase N+1 in StartupJobRunner).
    // ChatResumptionRunner must unconditionally skip ideation to prevent double-spawn.
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        is_handled,
        "Ideation should be handled by dedicated recovery loop, not ChatResumptionRunner"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_project() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create an interrupted conversation for Project
    let project_id = crate::domain::entities::ProjectId::new();
    let mut conv = ChatConversation::new_project(project_id);
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Project should NOT be handled by task resumption
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        !is_handled,
        "Project should NOT be handled by StartupJobRunner"
    );
}

async fn create_terminal_state_test(status: InternalStatus) -> bool {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), format!("{:?} Task", status));
    task.internal_status = status;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let mut conv = ChatConversation::new_task_execution(task_id);
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);
    runner.is_handled_by_task_resumption(&interrupted).await
}

#[tokio::test]
async fn test_is_handled_for_merged_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Merged).await;
    assert!(is_handled, "Merged task should be skipped (terminal state)");
}

#[tokio::test]
async fn test_is_handled_for_failed_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Failed).await;
    assert!(is_handled, "Failed task should be skipped (terminal state)");
}

#[tokio::test]
async fn test_is_handled_for_cancelled_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Cancelled).await;
    assert!(
        is_handled,
        "Cancelled task should be skipped (terminal state)"
    );
}

#[tokio::test]
async fn test_is_handled_for_stopped_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Stopped).await;
    assert!(
        is_handled,
        "Stopped task should be skipped (terminal state)"
    );
}
