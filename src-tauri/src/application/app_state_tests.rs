use super::*;
use crate::commands::ExecutionState;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, AgenticClient, ClientType, LogicalEffort,
};
use crate::domain::entities::{
    ChatMessage, IdeationSession, InternalStatus, Priority, Project, ProjectId, ProposalCategory,
    Task, TaskProposal,
};
use crate::infrastructure::{MockAgenticClient, MockCallType};

#[tokio::test]
async fn test_new_test_creates_empty_repositories() {
    let state = AppState::new_test();

    // Task repo should be empty
    let project_id = ProjectId::new();
    let tasks = state.task_repo.get_by_project(&project_id).await.unwrap();
    assert!(tasks.is_empty());

    // Project repo should be empty
    let projects = state.project_repo.get_all().await.unwrap();
    assert!(projects.is_empty());
}

#[tokio::test]
async fn test_with_repos_uses_custom_repositories() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    // Pre-populate the repos
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    project_repo.create(project.clone()).await.unwrap();

    let task = Task::new(project.id.clone(), "Test Task".to_string());
    task_repo.create(task.clone()).await.unwrap();

    // Create AppState with these repos
    let state = AppState::with_repos(task_repo, project_repo);

    // Verify the state uses our repos
    let projects = state.project_repo.get_all().await.unwrap();
    assert_eq!(projects.len(), 1);

    let tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(tasks.len(), 1);
}

#[tokio::test]
async fn test_task_and_project_repos_work_together() {
    let state = AppState::new_test();

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    state.project_repo.create(project.clone()).await.unwrap();

    // Create tasks for that project
    let task1 = Task::new(project.id.clone(), "Task 1".to_string());
    let task2 = Task::new(project.id.clone(), "Task 2".to_string());
    state.task_repo.create(task1).await.unwrap();
    state.task_repo.create(task2).await.unwrap();

    // Verify we can retrieve them
    let tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(tasks.len(), 2);
}

#[tokio::test]
async fn test_repositories_are_thread_safe() {
    let state = Arc::new(AppState::new_test());

    // Create a project first
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    state.project_repo.create(project.clone()).await.unwrap();

    // Spawn multiple tasks that use the repos concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let state_clone = Arc::clone(&state);
        let project_id = project.id.clone();
        handles.push(tokio::spawn(async move {
            let task = Task::new(project_id, format!("Task {}", i));
            state_clone.task_repo.create(task).await
        }));
    }

    // Wait for all to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    // Verify all tasks were created
    let tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(tasks.len(), 10);
}

#[tokio::test]
async fn test_new_test_creates_mock_agent_client() {
    let state = AppState::new_test();

    assert_eq!(
        state.agent_clients.default_harness,
        AgentHarnessKind::Claude
    );

    // Agent client should be mock and available
    let available = state
        .agent_clients
        .default_client
        .is_available()
        .await
        .unwrap();
    assert!(available);

    // Check capabilities indicate mock
    let caps = state.agent_clients.default_client.capabilities();
    assert_eq!(caps.client_type, ClientType::Mock);
}

#[tokio::test]
async fn test_with_agent_client_swaps_client() {
    let state = AppState::new_test();

    // Default is mock
    assert_eq!(
        state
            .agent_clients
            .default_client
            .capabilities()
            .client_type,
        ClientType::Mock
    );

    // Create custom mock with different capabilities wouldn't show,
    // but we can test the swap mechanism works
    let custom_mock = Arc::new(MockAgenticClient::new());
    let _state = state.with_agent_client(custom_mock);

    // If it compiled and ran, the swap worked
}

#[tokio::test]
async fn test_with_harness_agent_client_registers_specific_client() {
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state =
        AppState::new_test().with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());

    let resolved = state.resolve_harness_agent_client(AgentHarnessKind::Codex);

    assert_eq!(resolved.capabilities().client_type, ClientType::Mock);
    assert!(Arc::ptr_eq(&resolved, &codex_mock));
}

#[tokio::test]
async fn test_build_transition_service_with_execution_state_uses_app_agent_client() {
    let mock = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(mock.clone());
    let service =
        state.build_transition_service_with_execution_state(Arc::new(ExecutionState::new()));

    let repo_dir = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git name");
    std::fs::write(repo_dir.path().join("README.md"), "# test").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir.path())
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git commit");

    let project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(repo_dir.path().to_string_lossy().into_owned());
    state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task(&task.id, InternalStatus::QaRefining)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::QaRefining);

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
    match &calls[0].call_type {
        MockCallType::Spawn { role, prompt } => {
            assert_eq!(*role, crate::domain::agents::AgentRole::QaRefiner);
            assert!(prompt.contains(task.id.as_str()));
        }
        other => panic!("expected spawn call, got {other:?}"),
    }
}

#[tokio::test]
async fn test_build_transition_service_with_execution_state_uses_app_codex_client_for_codex_lane() {
    let default_mock = Arc::new(MockAgenticClient::new());
    let codex_mock = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_mock.clone())
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());
    let service =
        state.build_transition_service_with_execution_state(Arc::new(ExecutionState::new()));

    let repo_dir = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git name");
    std::fs::write(repo_dir.path().join("README.md"), "# test").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir.path())
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git commit");

    let project = Project::new(
        "Codex Project".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .agent_lane_settings_repo
        .upsert_for_project(project.id.as_str(), AgentLane::ExecutionWorker, &codex_lane)
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Codex Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(repo_dir.path().to_string_lossy().into_owned());
    state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task(&task.id, InternalStatus::QaRefining)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::QaRefining);
    assert!(
        default_mock.get_spawn_calls().await.is_empty(),
        "default client should not receive spawn calls when execution lane resolves to Codex"
    );

    let calls = codex_mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
    match &calls[0].call_type {
        MockCallType::Spawn { role, prompt } => {
            assert_eq!(*role, crate::domain::agents::AgentRole::QaRefiner);
            assert!(prompt.contains(task.id.as_str()));
        }
        other => panic!("expected spawn call, got {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_ideation_background_agent_runtime_uses_registered_harness_client() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());

    let project = Project::new("Codex Ideation Project".to_string(), "/tmp".to_string());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .agent_lane_settings_repo
        .upsert_for_project(project.id.as_str(), AgentLane::IdeationPrimary, &codex_lane)
        .await
        .unwrap();

    let runtime = state
        .resolve_ideation_background_agent_runtime(Some(project.id.as_str()))
        .await
        .expect("codex runtime should resolve");

    assert!(Arc::ptr_eq(&runtime.client, &codex_mock));
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(runtime.logical_effort, Some(LogicalEffort::XHigh));
}

#[tokio::test]
async fn test_resolve_ideation_background_agent_runtime_errors_without_registered_harness_client() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test().with_agent_client(default_mock.clone());
    state.agent_clients.harness_clients.clear();

    let project = Project::new("Fallback Ideation Project".to_string(), "/tmp".to_string());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .agent_lane_settings_repo
        .upsert_for_project(project.id.as_str(), AgentLane::IdeationPrimary, &codex_lane)
        .await
        .unwrap();

    let error = match state
        .resolve_ideation_background_agent_runtime(Some(project.id.as_str()))
        .await
    {
        Ok(_) => panic!("missing codex runtime should fail instead of falling back"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("Configured ideation sidecar harness unavailable"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn test_resolve_session_namer_runtime_uses_default_client_even_when_ideation_lane_is_codex() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_mock.clone())
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock);

    let project = Project::new("Codex Ideation Project".to_string(), "/tmp".to_string());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .agent_lane_settings_repo
        .upsert_for_project(project.id.as_str(), AgentLane::IdeationPrimary, &codex_lane)
        .await
        .unwrap();

    let runtime = state.resolve_session_namer_runtime().await;

    assert!(
        Arc::ptr_eq(&runtime.client, &default_mock),
        "session namer should stay on the default helper client instead of inheriting the main ideation lane"
    );
    assert_eq!(
        runtime.harness, None,
        "default helper client should not advertise an explicit non-default harness"
    );
    assert_eq!(runtime.model, None);
    assert_eq!(runtime.logical_effort, None);
}

#[tokio::test]
async fn test_ideation_repos_accessible() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create an ideation session
    let session = IdeationSession::new_with_title(project_id.clone(), "Test Session");
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();

    // Verify we can retrieve it
    let retrieved = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap();
    assert!(retrieved.is_some());

    // Create a proposal
    let proposal = TaskProposal::new(
        session_id.clone(),
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let proposal_id = proposal.id.clone();
    state.task_proposal_repo.create(proposal).await.unwrap();

    // Verify we can retrieve proposals
    let proposals = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .unwrap();
    assert_eq!(proposals.len(), 1);

    // Create a chat message
    let message = ChatMessage::user_in_session(session_id.clone(), "Hello");
    state.chat_message_repo.create(message).await.unwrap();

    // Verify we can retrieve messages
    let messages = state
        .chat_message_repo
        .get_by_session(&session_id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);

    // Add a dependency
    let proposal2 = TaskProposal::new(
        session_id.clone(),
        "Another Proposal",
        ProposalCategory::Feature,
        Priority::Low,
    );
    let proposal2_id = proposal2.id.clone();
    state.task_proposal_repo.create(proposal2).await.unwrap();

    state
        .proposal_dependency_repo
        .add_dependency(&proposal_id, &proposal2_id, None, None)
        .await
        .unwrap();

    let deps = state
        .proposal_dependency_repo
        .get_dependencies(&proposal_id)
        .await
        .unwrap();
    assert_eq!(deps.len(), 1);
}

#[tokio::test]
async fn test_task_dependency_repo_accessible() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create two tasks
    let task1 = Task::new(project_id.clone(), "Task 1".to_string());
    let task2 = Task::new(project_id.clone(), "Task 2".to_string());

    let task1_id = task1.id.clone();
    let task2_id = task2.id.clone();

    state.task_repo.create(task1).await.unwrap();
    state.task_repo.create(task2).await.unwrap();

    // Add a dependency
    state
        .task_dependency_repo
        .add_dependency(&task1_id, &task2_id)
        .await
        .unwrap();

    // Verify the dependency exists
    let has_dep = state
        .task_dependency_repo
        .has_dependency(&task1_id, &task2_id)
        .await
        .unwrap();
    assert!(has_dep);

    let blockers = state
        .task_dependency_repo
        .get_blockers(&task1_id)
        .await
        .unwrap();
    assert_eq!(blockers.len(), 1);
}

#[tokio::test]
async fn test_extensibility_repos_accessible() {
    use crate::domain::entities::methodology::MethodologyExtension;
    use crate::domain::entities::research::{ResearchBrief, ResearchProcess};
    use crate::domain::entities::status::InternalStatus;
    use crate::domain::entities::{
        Artifact, ArtifactBucket, ArtifactFlow, ArtifactFlowTrigger, ArtifactType, WorkflowColumn,
        WorkflowSchema,
    };

    let state = AppState::new_test();

    // Test workflow repository
    let workflow = WorkflowSchema::new(
        "Test Workflow",
        vec![
            WorkflowColumn::new("backlog", "Backlog", InternalStatus::Backlog),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    );
    state.workflow_repo.create(workflow.clone()).await.unwrap();
    let found_workflow = state.workflow_repo.get_by_id(&workflow.id).await.unwrap();
    assert!(found_workflow.is_some());

    // Test artifact repository
    let artifact = Artifact::new_inline("Test", ArtifactType::Prd, "content", "user");
    state.artifact_repo.create(artifact.clone()).await.unwrap();
    let found_artifact = state.artifact_repo.get_by_id(&artifact.id).await.unwrap();
    assert!(found_artifact.is_some());

    // Test artifact bucket repository
    let bucket = ArtifactBucket::new("Test Bucket")
        .accepts(ArtifactType::Prd)
        .with_writer("user");
    state
        .artifact_bucket_repo
        .create(bucket.clone())
        .await
        .unwrap();
    let found_bucket = state
        .artifact_bucket_repo
        .get_by_id(&bucket.id)
        .await
        .unwrap();
    assert!(found_bucket.is_some());

    // Test artifact flow repository
    let flow = ArtifactFlow::new("Test Flow", ArtifactFlowTrigger::on_artifact_created());
    state.artifact_flow_repo.create(flow.clone()).await.unwrap();
    let found_flow = state.artifact_flow_repo.get_by_id(&flow.id).await.unwrap();
    assert!(found_flow.is_some());

    // Test process repository
    let brief = ResearchBrief::new("Test question");
    let process = ResearchProcess::new("Test Research", brief, "researcher");
    state.process_repo.create(process.clone()).await.unwrap();
    let found_process = state.process_repo.get_by_id(&process.id).await.unwrap();
    assert!(found_process.is_some());

    // Test methodology repository
    let methodology = MethodologyExtension::new("Test Method", workflow);
    state
        .methodology_repo
        .create(methodology.clone())
        .await
        .unwrap();
    let found_methodology = state
        .methodology_repo
        .get_by_id(&methodology.id)
        .await
        .unwrap();
    assert!(found_methodology.is_some());
}
