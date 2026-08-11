use ralphx_lib::application::AppState;
use ralphx_lib::commands::ideation_commands::*;
use ralphx_lib::domain::entities::ideation::VerificationStatus;
use ralphx_lib::domain::entities::{
    ChatMessage, IdeationSession, IdeationSessionId, IdeationSessionStatus, Priority, Project,
    ProjectId, ProposalCategory, TaskProposal, TaskProposalId,
};
use ralphx_lib::domain::ideation::IdeationSettings;
use tauri::Manager;

fn setup_test_state() -> AppState {
    AppState::new_test()
}

#[tokio::test]
async fn test_create_ideation_session_without_title() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id.clone());
    let created = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    assert_eq!(created.project_id, project_id);
    assert!(created.title.is_none());
    assert_eq!(created.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_create_ideation_session_with_title() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    let session = IdeationSession::new_with_title(project_id.clone(), "Test Session");
    let created = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    assert_eq!(created.project_id, project_id);
    assert_eq!(created.title, Some("Test Session".to_string()));
    assert_eq!(created.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_get_ideation_session_returns_none_for_nonexistent() {
    let state = setup_test_state();
    let id = IdeationSessionId::new();

    let result = state
        .ideation_session_repo
        .get_by_id(&id)
        .await
        .expect("Failed to get ideation session by id in test");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_ideation_session_returns_existing() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let result = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .expect("Failed to get ideation session by id in test");
    assert!(result.is_some());
    assert_eq!(result.expect("Expected to find session").id, created.id);
}

#[tokio::test]
async fn test_list_ideation_sessions_by_project() {
    let state = setup_test_state();
    let project_id = ProjectId::new();
    let other_project_id = ProjectId::new();

    // Create sessions for our project
    state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("Failed to create ideation session in test");
    state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("Failed to create ideation session in test");

    // Create session for different project
    state
        .ideation_session_repo
        .create(IdeationSession::new(other_project_id))
        .await
        .expect("Failed to create ideation session in test");

    let sessions = state
        .ideation_session_repo
        .get_by_project(&project_id)
        .await
        .expect("Failed to get sessions by project in test");
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_archive_ideation_session() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Archived)
        .await
        .expect("Failed to update ideation session status in test");

    let retrieved = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .expect("Failed to get by id in test")
        .expect("Expected to find entity");
    assert_eq!(retrieved.status, IdeationSessionStatus::Archived);
}

#[tokio::test]
async fn test_delete_ideation_session() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    state
        .ideation_session_repo
        .delete(&created.id)
        .await
        .expect("Failed to delete ideation session in test");

    let result = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .expect("Failed to get by id in test");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_session_response_serialization() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new_with_title(project_id, "Test Session");
    let response = IdeationSessionResponse::from(session);

    assert!(!response.id.is_empty());
    assert_eq!(response.title, Some("Test Session".to_string()));
    assert_eq!(response.status, "active");

    // Verify it serializes to JSON
    let json = serde_json::to_string(&response).expect("Failed to serialize response in test");
    assert!(json.contains("\"title\":\"Test Session\""));
}

#[tokio::test]
async fn test_get_session_with_data() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create proposal for session
    let proposal = TaskProposal::new(
        created_session.id.clone(),
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::High,
    );
    state
        .task_proposal_repo
        .create(proposal)
        .await
        .expect("Failed to create task proposal in test");

    // Create message for session
    let message = ChatMessage::user_in_session(created_session.id.clone(), "Hello");
    state
        .chat_message_repo
        .create(message)
        .await
        .expect("Failed to create chat message in test");

    // Get session with data
    let proposals = state
        .task_proposal_repo
        .get_by_session(&created_session.id)
        .await
        .expect("Failed to get proposals by session in test");
    let messages = state
        .chat_message_repo
        .get_by_session(&created_session.id)
        .await
        .expect("Failed to get messages by session in test");

    assert_eq!(proposals.len(), 1);
    assert_eq!(messages.len(), 1);
}

#[tokio::test]
async fn test_proposal_response_serialization() {
    let session_id = IdeationSessionId::new();
    let proposal = TaskProposal::new(
        session_id,
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::High,
    );
    let response = TaskProposalResponse::from(proposal);

    assert!(!response.id.is_empty());
    assert_eq!(response.title, "Test Proposal");
    assert_eq!(response.category, "feature");
    assert_eq!(response.suggested_priority, "high");

    // Verify it serializes to JSON
    let json = serde_json::to_string(&response).expect("Failed to serialize response in test");
    assert!(json.contains("\"title\":\"Test Proposal\""));
}

#[tokio::test]
async fn test_message_response_serialization() {
    let session_id = IdeationSessionId::new();
    let message = ChatMessage::user_in_session(session_id, "Hello world");
    let response = ChatMessageResponse::from(message);

    assert!(!response.id.is_empty());
    assert_eq!(response.content, "Hello world");
    assert_eq!(response.role, "user");

    // Verify it serializes to JSON
    let json = serde_json::to_string(&response).expect("Failed to serialize response in test");
    assert!(json.contains("\"content\":\"Hello world\""));
}

// ========================================================================
// Task Proposal Tests
// ========================================================================

#[tokio::test]
async fn test_create_task_proposal() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session first
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create proposal
    let proposal = TaskProposal::new(
        created_session.id.clone(),
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::High,
    );
    let created = state
        .task_proposal_repo
        .create(proposal)
        .await
        .expect("Failed to create task proposal in test");

    assert_eq!(created.title, "Test Proposal");
    assert_eq!(created.category, ProposalCategory::Feature);
    assert_eq!(created.suggested_priority, Priority::High);
}

#[tokio::test]
async fn test_get_task_proposal_returns_none_for_nonexistent() {
    let state = setup_test_state();
    let id = TaskProposalId::new();

    let result = state
        .task_proposal_repo
        .get_by_id(&id)
        .await
        .expect("Failed to get task proposal by id in test");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_list_session_proposals() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create proposals
    for i in 0..3 {
        let proposal = TaskProposal::new(
            created_session.id.clone(),
            format!("Proposal {}", i),
            ProposalCategory::Feature,
            Priority::Medium,
        );
        state
            .task_proposal_repo
            .create(proposal)
            .await
            .expect("Failed to create task proposal in test");
    }

    let proposals = state
        .task_proposal_repo
        .get_by_session(&created_session.id)
        .await
        .expect("Failed to get proposals by session in test");
    assert_eq!(proposals.len(), 3);
}

#[tokio::test]
async fn test_update_task_proposal() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session and proposal
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let proposal = TaskProposal::new(
        created_session.id.clone(),
        "Original Title",
        ProposalCategory::Feature,
        Priority::Low,
    );
    let created = state
        .task_proposal_repo
        .create(proposal)
        .await
        .expect("Failed to create task proposal in test");

    // Update proposal
    let mut updated = created.clone();
    updated.title = "Updated Title".to_string();
    updated.user_modified = true;

    state
        .task_proposal_repo
        .update(&updated)
        .await
        .expect("Failed to update task proposal in test");

    let retrieved = state
        .task_proposal_repo
        .get_by_id(&created.id)
        .await
        .expect("Failed to get by id in test")
        .expect("Expected to find entity");
    assert_eq!(retrieved.title, "Updated Title");
    assert!(retrieved.user_modified);
}

#[tokio::test]
async fn test_archive_task_proposal_soft_deletes() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session and proposal
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let proposal = TaskProposal::new(
        created_session.id.clone(),
        "To Archive",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let created = state
        .task_proposal_repo
        .create(proposal)
        .await
        .expect("Failed to create task proposal in test");

    // Archive proposal
    state
        .task_proposal_repo
        .archive(&created.id)
        .await
        .expect("Failed to archive task proposal in test");

    let result = state
        .task_proposal_repo
        .get_by_id(&created.id)
        .await
        .expect("Failed to get by id in test")
        .expect("Expected entity to still exist after archive");
    assert!(result.archived_at.is_some());
}

#[tokio::test]
async fn test_toggle_proposal_selection() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session and proposal
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let proposal = TaskProposal::new(
        created_session.id.clone(),
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let created = state
        .task_proposal_repo
        .create(proposal)
        .await
        .expect("Failed to create task proposal in test");

    // Initial state should be selected (false)
    assert!(!created.selected);

    // Toggle to true
    state
        .task_proposal_repo
        .update_selection(&created.id, true)
        .await
        .expect("Failed to update selection in test");

    let retrieved = state
        .task_proposal_repo
        .get_by_id(&created.id)
        .await
        .expect("Failed to get by id in test")
        .expect("Expected to find entity");
    assert!(retrieved.selected);
}

#[tokio::test]
async fn test_reorder_proposals() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create 3 proposals
    let mut ids = Vec::new();
    for i in 0..3 {
        let proposal = TaskProposal::new(
            created_session.id.clone(),
            format!("Proposal {}", i),
            ProposalCategory::Feature,
            Priority::Medium,
        );
        let created = state
            .task_proposal_repo
            .create(proposal)
            .await
            .expect("Failed to create task proposal in test");
        ids.push(created.id);
    }

    // Reverse the order
    let reversed_ids: Vec<TaskProposalId> = ids.into_iter().rev().collect();
    state
        .task_proposal_repo
        .reorder(&created_session.id, reversed_ids)
        .await
        .expect("Failed to reorder proposals in test");

    // Verify order changed
    let proposals = state
        .task_proposal_repo
        .get_by_session(&created_session.id)
        .await
        .expect("Failed to get proposals by session in test");
    assert_eq!(proposals.len(), 3);
    // The first proposal should now be "Proposal 2"
    assert_eq!(proposals[0].title, "Proposal 2");
}

#[tokio::test]
async fn test_priority_assessment_response() {
    let session_id = IdeationSessionId::new();
    let proposal = TaskProposal::new(
        session_id,
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::Critical,
    );

    let response = PriorityAssessmentResponse {
        proposal_id: proposal.id.as_str().to_string(),
        priority: proposal.suggested_priority.to_string(),
        score: proposal.priority_score,
        reason: "Test reason".to_string(),
    };

    assert_eq!(response.priority, "critical");
    assert_eq!(response.reason, "Test reason");

    // Verify it serializes to JSON
    let json = serde_json::to_string(&response).expect("Failed to serialize response in test");
    assert!(json.contains("\"priority\":\"critical\""));
}

// ========================================================================
// Dependency and Apply Tests
// ========================================================================

#[tokio::test]
async fn test_add_proposal_dependency() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session and proposals
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let proposal1 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 1",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let proposal2 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 2",
        ProposalCategory::Feature,
        Priority::Medium,
    );

    let p1 = state
        .task_proposal_repo
        .create(proposal1)
        .await
        .expect("Failed to create task proposal in test");
    let p2 = state
        .task_proposal_repo
        .create(proposal2)
        .await
        .expect("Failed to create task proposal in test");

    // Add dependency: p1 depends on p2
    state
        .proposal_dependency_repo
        .add_dependency(&p1.id, &p2.id, None, None)
        .await
        .expect("Failed to add dependency in test");

    // Verify dependency exists
    let deps = state
        .proposal_dependency_repo
        .get_dependencies(&p1.id)
        .await
        .expect("Failed to get dependencies in test");
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0], p2.id);
}

#[tokio::test]
async fn test_remove_proposal_dependency() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session and proposals
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let proposal1 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 1",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let proposal2 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 2",
        ProposalCategory::Feature,
        Priority::Medium,
    );

    let p1 = state
        .task_proposal_repo
        .create(proposal1)
        .await
        .expect("Failed to create task proposal in test");
    let p2 = state
        .task_proposal_repo
        .create(proposal2)
        .await
        .expect("Failed to create task proposal in test");

    // Add then remove dependency
    state
        .proposal_dependency_repo
        .add_dependency(&p1.id, &p2.id, None, None)
        .await
        .expect("Failed to add dependency in test");
    state
        .proposal_dependency_repo
        .remove_dependency(&p1.id, &p2.id)
        .await
        .expect("Failed to remove dependency in test");

    // Verify dependency was removed
    let deps = state
        .proposal_dependency_repo
        .get_dependencies(&p1.id)
        .await
        .expect("Failed to get dependencies in test");
    assert!(deps.is_empty());
}

#[tokio::test]
async fn test_get_proposal_dependents() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session and proposals
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let proposal1 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 1",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let proposal2 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 2",
        ProposalCategory::Feature,
        Priority::Medium,
    );

    let p1 = state
        .task_proposal_repo
        .create(proposal1)
        .await
        .expect("Failed to create task proposal in test");
    let p2 = state
        .task_proposal_repo
        .create(proposal2)
        .await
        .expect("Failed to create task proposal in test");

    // p1 depends on p2, so p2 should have p1 as a dependent
    state
        .proposal_dependency_repo
        .add_dependency(&p1.id, &p2.id, None, None)
        .await
        .expect("Failed to add dependency in test");

    let dependents = state
        .proposal_dependency_repo
        .get_dependents(&p2.id)
        .await
        .expect("Failed to get dependents in test");
    assert_eq!(dependents.len(), 1);
    assert_eq!(dependents[0], p1.id);
}

#[tokio::test]
async fn test_analyze_dependencies_empty_session() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session with no proposals
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Get dependencies (should be empty graph)
    let proposals = state
        .task_proposal_repo
        .get_by_session(&created_session.id)
        .await
        .expect("Failed to get proposals by session in test");
    let deps = state
        .proposal_dependency_repo
        .get_all_for_session(&created_session.id)
        .await
        .expect("Failed to get all dependencies for session in test");

    let graph = build_dependency_graph(&proposals, &deps);

    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());
    assert!(!graph.has_cycles);
}

#[tokio::test]
async fn test_analyze_dependencies_acknowledges_session_for_accept_gate() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let proposal1 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 1",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let proposal2 = TaskProposal::new(
        created_session.id.clone(),
        "Proposal 2",
        ProposalCategory::Feature,
        Priority::Medium,
    );

    state
        .task_proposal_repo
        .create(proposal1)
        .await
        .expect("Failed to create first task proposal in test");
    state
        .task_proposal_repo
        .create(proposal2)
        .await
        .expect("Failed to create second task proposal in test");

    let before = state
        .ideation_session_repo
        .get_by_id(&created_session.id)
        .await
        .expect("Failed to load ideation session before analysis")
        .expect("Expected ideation session to exist before analysis");
    assert!(
        !before.dependencies_acknowledged,
        "precondition: session must start unacknowledged"
    );

    analyze_dependencies_for_session(&state, &created_session.id)
        .await
        .expect("analyze_dependencies_for_session should succeed");

    let after = state
        .ideation_session_repo
        .get_by_id(&created_session.id)
        .await
        .expect("Failed to load ideation session after analysis")
        .expect("Expected ideation session to exist after analysis");
    assert!(
        after.dependencies_acknowledged,
        "dependency analysis must acknowledge the session for the accept gate"
    );
}

#[tokio::test]
async fn test_dependency_graph_response_serialization() {
    use ralphx_lib::domain::entities::DependencyGraph;

    let graph = DependencyGraph::new();
    let response = DependencyGraphResponse::from(graph);

    assert!(response.nodes.is_empty());
    assert!(response.edges.is_empty());
    assert!(!response.has_cycles);

    // Verify it serializes to JSON
    let json = serde_json::to_string(&response).expect("Failed to serialize response in test");
    assert!(json.contains("\"has_cycles\":false"));
}

#[tokio::test]
async fn test_task_blockers() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create tasks
    let task1 = ralphx_lib::domain::entities::Task::new(project_id.clone(), "Task 1".to_string());
    let task2 = ralphx_lib::domain::entities::Task::new(project_id.clone(), "Task 2".to_string());

    let t1 = state
        .task_repo
        .create(task1)
        .await
        .expect("Failed to create task in test");
    let t2 = state
        .task_repo
        .create(task2)
        .await
        .expect("Failed to create task in test");

    // Add dependency: t1 depends on t2
    state
        .task_dependency_repo
        .add_dependency(&t1.id, &t2.id)
        .await
        .expect("Failed to add task dependency in test");

    // t2 should be a blocker for t1
    let blockers = state
        .task_dependency_repo
        .get_blockers(&t1.id)
        .await
        .expect("Failed to get blockers in test");
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0], t2.id);

    // t1 should be blocked by t2
    let blocked = state
        .task_dependency_repo
        .get_blocked_by(&t2.id)
        .await
        .expect("Failed to get blocked tasks in test");
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0], t1.id);
}

// ========================================================================
// Chat Message Tests
// ========================================================================

#[tokio::test]
async fn test_send_chat_message_to_session() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session first
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Send a message
    let message = ChatMessage::user_in_session(created_session.id.clone(), "Hello world");
    let created = state
        .chat_message_repo
        .create(message)
        .await
        .expect("Failed to create chat message in test");

    assert_eq!(created.content, "Hello world");
    assert_eq!(created.session_id, Some(created_session.id));
}

#[tokio::test]
async fn test_send_chat_message_to_project() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Send a message to project
    let message = ChatMessage::user_in_project(project_id.clone(), "Project message");
    let created = state
        .chat_message_repo
        .create(message)
        .await
        .expect("Failed to create chat message in test");

    assert_eq!(created.content, "Project message");
    assert_eq!(created.project_id, Some(project_id));
    assert!(created.session_id.is_none());
}

#[tokio::test]
async fn test_send_chat_message_about_task() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create a task
    let task = ralphx_lib::domain::entities::Task::new(project_id, "Test Task".to_string());
    let created_task = state
        .task_repo
        .create(task)
        .await
        .expect("Failed to create task in test");

    // Send a message about the task
    let message = ChatMessage::user_about_task(created_task.id.clone(), "Task message");
    let created = state
        .chat_message_repo
        .create(message)
        .await
        .expect("Failed to create chat message in test");

    assert_eq!(created.content, "Task message");
    assert_eq!(created.task_id, Some(created_task.id));
}

#[tokio::test]
async fn test_get_session_messages() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Send multiple messages
    for i in 1..=3 {
        let message =
            ChatMessage::user_in_session(created_session.id.clone(), format!("Message {}", i));
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("Failed to create chat message in test");
    }

    // Get all messages
    let messages = state
        .chat_message_repo
        .get_by_session(&created_session.id)
        .await
        .expect("Failed to get messages by session in test");
    assert_eq!(messages.len(), 3);
}

#[tokio::test]
async fn test_get_project_messages() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Send messages to project
    for i in 1..=2 {
        let message =
            ChatMessage::user_in_project(project_id.clone(), format!("Project message {}", i));
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("Failed to create chat message in test");
    }

    // Get all project messages
    let messages = state
        .chat_message_repo
        .get_by_project(&project_id)
        .await
        .expect("Failed to get sessions by project in test");
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_get_task_messages() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create a task
    let task = ralphx_lib::domain::entities::Task::new(project_id, "Test Task".to_string());
    let created_task = state
        .task_repo
        .create(task)
        .await
        .expect("Failed to create task in test");

    // Send messages about the task
    for i in 1..=2 {
        let message =
            ChatMessage::user_about_task(created_task.id.clone(), format!("Task message {}", i));
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("Failed to create chat message in test");
    }

    // Get all task messages
    let messages = state
        .chat_message_repo
        .get_by_task(&created_task.id)
        .await
        .expect("Failed to get messages by task in test");
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_delete_chat_message() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session and message
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    let message = ChatMessage::user_in_session(created_session.id.clone(), "To delete");
    let created = state
        .chat_message_repo
        .create(message)
        .await
        .expect("Failed to create chat message in test");

    // Delete the message
    state
        .chat_message_repo
        .delete(&created.id)
        .await
        .expect("Failed to delete chat message in test");

    // Verify it's gone
    let result = state
        .chat_message_repo
        .get_by_id(&created.id)
        .await
        .expect("Failed to get chat message by id in test");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delete_session_messages() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create multiple messages
    for i in 1..=3 {
        let message =
            ChatMessage::user_in_session(created_session.id.clone(), format!("Message {}", i));
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("Failed to create chat message in test");
    }

    // Delete all session messages
    state
        .chat_message_repo
        .delete_by_session(&created_session.id)
        .await
        .expect("Failed to delete messages by session in test");

    // Verify they're gone
    let messages = state
        .chat_message_repo
        .get_by_session(&created_session.id)
        .await
        .expect("Failed to get messages by session in test");
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_count_session_messages() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create messages
    for i in 1..=5 {
        let message =
            ChatMessage::user_in_session(created_session.id.clone(), format!("Message {}", i));
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("Failed to create chat message in test");
    }

    // Count messages
    let count = state
        .chat_message_repo
        .count_by_session(&created_session.id)
        .await
        .expect("Failed to count messages by session in test");
    assert_eq!(count, 5);
}

#[tokio::test]
async fn test_get_recent_session_messages() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create 5 messages
    for i in 1..=5 {
        let message =
            ChatMessage::user_in_session(created_session.id.clone(), format!("Message {}", i));
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("Failed to create chat message in test");
    }

    // Get only 3 recent messages
    let messages = state
        .chat_message_repo
        .get_recent_by_session(&created_session.id, 3)
        .await
        .expect("Failed to get recent messages by session in test");
    assert_eq!(messages.len(), 3);
}

#[tokio::test]
async fn test_chat_message_response_includes_all_fields() {
    let session_id = IdeationSessionId::new();
    let mut message = ChatMessage::user_in_session(session_id.clone(), "Test message");
    message.metadata = Some(r#"{"key": "value"}"#.to_string());

    let response = ChatMessageResponse::from(message.clone());

    assert_eq!(response.content, "Test message");
    assert_eq!(response.role, "user");
    assert_eq!(response.session_id, Some(session_id.as_str().to_string()));
    assert!(response.project_id.is_none());
    assert!(response.task_id.is_none());
    assert_eq!(response.metadata, Some(r#"{"key": "value"}"#.to_string()));
}

#[tokio::test]
async fn test_orchestrator_message_in_session() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create orchestrator message
    let message = ChatMessage::orchestrator_in_session(created_session.id.clone(), "AI response");
    let created = state
        .chat_message_repo
        .create(message)
        .await
        .expect("Failed to create chat message in test");

    let response = ChatMessageResponse::from(created);
    assert_eq!(response.role, "orchestrator");
    assert_eq!(response.content, "AI response");
}

#[tokio::test]
async fn test_system_message_in_session() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create ideation session in test");

    // Create system message
    let message = ChatMessage::system_in_session(created_session.id.clone(), "Session started");
    let created = state
        .chat_message_repo
        .create(message)
        .await
        .expect("Failed to create chat message in test");

    let response = ChatMessageResponse::from(created);
    assert_eq!(response.role, "system");
    assert_eq!(response.content, "Session started");
}

// ========================================================================
// Ideation Settings Tests
// ========================================================================

#[tokio::test]
async fn ipc_contract_ideation_settings_commands_preserve_backend_tasks_state() {
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("ideation settings command app should build");

    let current = get_ideation_settings(app.state::<AppState>())
        .await
        .expect("settings command should return defaults");
    let backend_tasks_enabled = current.tasks_enabled;
    let backend_tasks_feature_state = current.tasks_feature_state;

    let requested = IdeationSettings {
        plan_mode: ralphx_lib::domain::ideation::IdeationPlanMode::Required,
        tasks_enabled: !backend_tasks_enabled,
        ..current
    };
    let updated = update_ideation_settings(requested, app.state::<AppState>())
        .await
        .expect("ordinary settings command should persist planning fields");

    assert_eq!(
        updated.plan_mode,
        ralphx_lib::domain::ideation::IdeationPlanMode::Required
    );
    assert_eq!(
        updated.tasks_enabled, backend_tasks_enabled,
        "ordinary settings updates must preserve backend-owned enablement"
    );
    assert_eq!(
        updated.tasks_feature_state, backend_tasks_feature_state,
        "ordinary settings updates must preserve backend-owned feature state"
    );
}

#[tokio::test]
async fn test_get_ideation_settings_returns_default() {
    let state = setup_test_state();

    // Get settings (should return default)
    let settings = state
        .ideation_settings_repo
        .get_settings()
        .await
        .expect("Failed to get ideation settings in test");

    assert_eq!(
        settings.plan_mode,
        ralphx_lib::domain::ideation::IdeationPlanMode::Optional
    );
    assert!(!settings.require_plan_approval);
    assert!(settings.suggest_plans_for_complex);
    assert!(settings.auto_link_proposals);
}

#[tokio::test]
async fn test_update_ideation_settings() {
    let state = setup_test_state();

    // Create custom settings
    let custom_settings = IdeationSettings {
        tasks_enabled: false,
        tasks_feature_state: Default::default(),
        plan_mode: ralphx_lib::domain::ideation::IdeationPlanMode::Required,
        require_plan_approval: true,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        auto_verify_plans: false,
        auto_verify_draft_plans: true,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        external_overrides: Default::default(),
    };

    // Update settings
    let updated = state
        .ideation_settings_repo
        .update_settings(&custom_settings)
        .await
        .expect("Failed to update ideation settings in test");

    assert_eq!(
        updated.plan_mode,
        ralphx_lib::domain::ideation::IdeationPlanMode::Required
    );
    assert!(updated.require_plan_approval);
    assert!(!updated.suggest_plans_for_complex);
    assert!(!updated.auto_link_proposals);
}

#[tokio::test]
async fn test_ideation_settings_persist_across_reads() {
    let state = setup_test_state();

    // Update settings
    let custom_settings = IdeationSettings {
        tasks_enabled: false,
        tasks_feature_state: Default::default(),
        plan_mode: ralphx_lib::domain::ideation::IdeationPlanMode::Parallel,
        require_plan_approval: false,
        suggest_plans_for_complex: true,
        auto_link_proposals: false,
        auto_verify_plans: false,
        auto_verify_draft_plans: true,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        external_overrides: Default::default(),
    };

    state
        .ideation_settings_repo
        .update_settings(&custom_settings)
        .await
        .expect("Failed to update ideation settings in test");

    // Read settings again
    let retrieved = state
        .ideation_settings_repo
        .get_settings()
        .await
        .expect("Failed to get ideation settings in test");

    assert_eq!(
        retrieved.plan_mode,
        ralphx_lib::domain::ideation::IdeationPlanMode::Parallel
    );
    assert!(!retrieved.require_plan_approval);
    assert!(retrieved.suggest_plans_for_complex);
    assert!(!retrieved.auto_link_proposals);
}

// ========================================================================
// Cascade Delete Session Tests (Phase 103)
// ========================================================================

#[tokio::test]
async fn test_delete_session_cascades_to_tasks() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session
    let session = IdeationSession::new(project_id.clone());
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Create tasks linked to this session
    let mut task1 =
        ralphx_lib::domain::entities::Task::new(project_id.clone(), "Task 1".to_string());
    task1.ideation_session_id = Some(created_session.id.clone());
    let mut task2 =
        ralphx_lib::domain::entities::Task::new(project_id.clone(), "Task 2".to_string());
    task2.ideation_session_id = Some(created_session.id.clone());

    let t1 = state
        .task_repo
        .create(task1)
        .await
        .expect("Failed to create task");
    let t2 = state
        .task_repo
        .create(task2)
        .await
        .expect("Failed to create task");

    // Verify tasks exist and are linked to session
    let session_tasks = state
        .task_repo
        .get_by_ideation_session(&created_session.id)
        .await
        .expect("Failed to query tasks");
    assert_eq!(session_tasks.len(), 2);

    // Simulate cascade: delete tasks then session (mirrors command logic)
    for task in &session_tasks {
        state
            .task_repo
            .delete(&task.id)
            .await
            .expect("Failed to delete task");
    }
    state
        .ideation_session_repo
        .delete(&created_session.id)
        .await
        .expect("Failed to delete session");

    // Verify tasks are gone
    assert!(state.task_repo.get_by_id(&t1.id).await.unwrap().is_none());
    assert!(state.task_repo.get_by_id(&t2.id).await.unwrap().is_none());

    // Verify session is gone
    assert!(state
        .ideation_session_repo
        .get_by_id(&created_session.id)
        .await
        .unwrap()
        .is_none());

    // Verify get_by_ideation_session returns empty
    let remaining = state
        .task_repo
        .get_by_ideation_session(&created_session.id)
        .await
        .unwrap();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn test_delete_session_with_no_tasks() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create session with no tasks
    let session = IdeationSession::new(project_id);
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Verify no tasks for this session
    let session_tasks = state
        .task_repo
        .get_by_ideation_session(&created_session.id)
        .await
        .unwrap();
    assert!(session_tasks.is_empty());

    // Delete session directly (no cascade needed)
    state
        .ideation_session_repo
        .delete(&created_session.id)
        .await
        .expect("Failed to delete session");

    // Verify session is gone
    assert!(state
        .ideation_session_repo
        .get_by_id(&created_session.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_delete_session_only_deletes_own_tasks() {
    let state = setup_test_state();
    let project_id = ProjectId::new();

    // Create two sessions
    let session_a = IdeationSession::new(project_id.clone());
    let created_a = state
        .ideation_session_repo
        .create(session_a)
        .await
        .expect("Failed to create session A");

    let session_b = IdeationSession::new(project_id.clone());
    let created_b = state
        .ideation_session_repo
        .create(session_b)
        .await
        .expect("Failed to create session B");

    // Create tasks: 2 for session A, 1 for session B
    let mut task_a1 =
        ralphx_lib::domain::entities::Task::new(project_id.clone(), "A-Task 1".to_string());
    task_a1.ideation_session_id = Some(created_a.id.clone());
    let mut task_a2 =
        ralphx_lib::domain::entities::Task::new(project_id.clone(), "A-Task 2".to_string());
    task_a2.ideation_session_id = Some(created_a.id.clone());
    let mut task_b1 =
        ralphx_lib::domain::entities::Task::new(project_id.clone(), "B-Task 1".to_string());
    task_b1.ideation_session_id = Some(created_b.id.clone());

    state.task_repo.create(task_a1).await.unwrap();
    state.task_repo.create(task_a2).await.unwrap();
    let tb1 = state.task_repo.create(task_b1).await.unwrap();

    // Cascade delete session A's tasks only
    let tasks_a = state
        .task_repo
        .get_by_ideation_session(&created_a.id)
        .await
        .unwrap();
    assert_eq!(tasks_a.len(), 2);

    for task in &tasks_a {
        state.task_repo.delete(&task.id).await.unwrap();
    }
    state
        .ideation_session_repo
        .delete(&created_a.id)
        .await
        .unwrap();

    // Session B's task should be untouched
    let tasks_b = state
        .task_repo
        .get_by_ideation_session(&created_b.id)
        .await
        .unwrap();
    assert_eq!(tasks_b.len(), 1);
    assert_eq!(tasks_b[0].title, "B-Task 1");

    // Session B's task still retrievable by ID
    assert!(state.task_repo.get_by_id(&tb1.id).await.unwrap().is_some());
}

// ========================================================================
// ExecutionPlan Integration Tests (Phase 46-48)
// ========================================================================

#[tokio::test]
async fn test_execution_plan_created_and_stored() {
    use ralphx_lib::domain::entities::{ExecutionPlan, ExecutionPlanStatus, IdeationSession};

    let state = setup_test_state();
    let project_id = ProjectId::new();

    // ExecutionPlan has FK on session_id — create session first
    let session = IdeationSession::new(project_id.clone());
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    let plan = ExecutionPlan::new(created_session.id.clone());
    let plan_id = plan.id.clone();

    let created = state
        .execution_plan_repo
        .create(plan)
        .await
        .expect("Failed to create execution plan");

    assert_eq!(created.id, plan_id);
    assert_eq!(created.session_id, created_session.id);
    assert_eq!(created.status, ExecutionPlanStatus::Active);
}

#[tokio::test]
async fn test_task_execution_plan_id_persists() {
    use ralphx_lib::domain::entities::{ExecutionPlan, ExecutionPlanId, IdeationSession};

    let state = setup_test_state();
    let project_id = ProjectId::new();

    // ExecutionPlan has FK on session_id — create session first
    let session = IdeationSession::new(project_id.clone());
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");
    let session_id = created_session.id.clone();

    // Create an ExecutionPlan
    let plan = ExecutionPlan::new(session_id.clone());
    let exec_plan_id: ExecutionPlanId = plan.id.clone();
    state
        .execution_plan_repo
        .create(plan)
        .await
        .expect("Failed to create execution plan");

    // Create a task with execution_plan_id set
    let mut task =
        ralphx_lib::domain::entities::Task::new(project_id.clone(), "EP Task".to_string());
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(exec_plan_id.clone());

    let created = state
        .task_repo
        .create(task.clone())
        .await
        .expect("Failed to create task");

    assert_eq!(created.execution_plan_id, Some(exec_plan_id.clone()));

    // Retrieve and verify field persists
    let fetched = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("get_by_id failed")
        .expect("task not found");

    assert_eq!(fetched.execution_plan_id, Some(exec_plan_id));
}

#[tokio::test]
async fn test_plan_branch_execution_plan_id_persists() {
    use ralphx_lib::domain::entities::{
        ArtifactId, ExecutionPlan, ExecutionPlanId, IdeationSession, PlanBranch,
    };

    let state = setup_test_state();
    let project_id = ProjectId::new();

    // ExecutionPlan has FK on session_id — create session first
    let session = IdeationSession::new(project_id.clone());
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");
    let session_id = created_session.id.clone();

    // Create an ExecutionPlan
    let plan = ExecutionPlan::new(session_id.clone());
    let exec_plan_id: ExecutionPlanId = plan.id.clone();
    state
        .execution_plan_repo
        .create(plan)
        .await
        .expect("Failed to create execution plan");

    // Create plan branch with execution_plan_id set
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-123"),
        session_id.clone(),
        project_id.clone(),
        format!("ralphx/test/plan-{}", &exec_plan_id.as_str()[..8]),
        "main".to_string(),
    );
    branch.execution_plan_id = Some(exec_plan_id.clone());

    state
        .plan_branch_repo
        .create(branch)
        .await
        .expect("Failed to create plan branch");

    // Lookup by execution_plan_id
    let found = state
        .plan_branch_repo
        .get_by_execution_plan_id(&exec_plan_id)
        .await
        .expect("get_by_execution_plan_id failed");

    assert!(found.is_some(), "Branch not found by execution_plan_id");
    let branch = found.unwrap();
    assert_eq!(branch.execution_plan_id, Some(exec_plan_id));
    assert_eq!(branch.session_id, session_id);
}

#[tokio::test]
async fn test_two_execution_plans_same_session_have_unique_ids() {
    use ralphx_lib::domain::entities::{ExecutionPlan, IdeationSession};

    let state = setup_test_state();
    let project_id = ProjectId::new();

    // ExecutionPlan has FK on session_id — create session first
    let session = IdeationSession::new(project_id.clone());
    let created_session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");
    let session_id = created_session.id.clone();

    let plan1 = ExecutionPlan::new(session_id.clone());
    let plan2 = ExecutionPlan::new(session_id.clone());

    let id1 = plan1.id.clone();
    let id2 = plan2.id.clone();

    state
        .execution_plan_repo
        .create(plan1)
        .await
        .expect("Failed to create plan1");
    state
        .execution_plan_repo
        .create(plan2)
        .await
        .expect("Failed to create plan2");

    assert_ne!(
        id1, id2,
        "Each re-accept must produce a unique ExecutionPlan ID"
    );
}

#[tokio::test]
async fn test_get_by_execution_plan_id_returns_none_when_absent() {
    use ralphx_lib::domain::entities::ExecutionPlanId;

    let state = setup_test_state();
    let nonexistent_id = ExecutionPlanId::new();

    let result = state
        .plan_branch_repo
        .get_by_execution_plan_id(&nonexistent_id)
        .await
        .expect("get_by_execution_plan_id should not error");

    assert!(result.is_none());
}

// ============================================================================
// apply_proposals_core regression tests
// ============================================================================

/// State factory for apply_proposals_core tests.
///
/// Uses `AppState::new_sqlite_for_apply_test()` so that all repositories accessed
/// by apply_proposals_core (both via async repo calls and via db.run_transaction)
/// share a single in-memory SQLite connection — ensuring rows written inside the
/// transaction are visible to subsequent repo reads in the same test.
fn setup_apply_test_state() -> AppState {
    AppState::new_sqlite_for_apply_test()
}

/// Helper: create a project and session with N proposals, return (project_id, session, proposal_ids)
async fn setup_session_with_proposals(
    state: &AppState,
    proposal_count: usize,
) -> (
    ProjectId,
    ralphx_lib::domain::entities::IdeationSession,
    Vec<String>,
) {
    use ralphx_lib::domain::entities::{Priority, Project, ProposalCategory};

    let repo_dir = setup_git_repo_for_apply_test();
    let repo_path = repo_dir.keep();
    let project = Project::new(
        "Test Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    let mut ids = Vec::new();
    for i in 0..proposal_count {
        let proposal = TaskProposal::new(
            session.id.clone(),
            format!("Proposal {}", i + 1),
            ProposalCategory::Feature,
            Priority::Medium,
        );
        let p = state
            .task_proposal_repo
            .create(proposal)
            .await
            .expect("Failed to create proposal");
        ids.push(p.id.as_str().to_string());
    }

    (project.id, session, ids)
}

#[tokio::test]
async fn test_apply_proposals_core_creates_tasks_with_ready_status() {
    use ralphx_lib::domain::entities::InternalStatus;

    let state = setup_apply_test_state();
    let (project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    // Acknowledge dependencies (required by gate for multi-proposal sessions)
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to set dependencies_acknowledged");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: proposal_ids.clone(),
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed");

    assert_eq!(result.created_task_ids.len(), 2, "Should create 2 tasks");
    assert_eq!(result.dependencies_created, 0);
    assert!(result.warnings.is_empty());
    assert_eq!(result.project_id, project_id.as_str());
    assert_eq!(result.session_id, session.id.as_str());
    assert!(
        result.any_ready_tasks,
        "Tasks with no blockers should be Ready"
    );

    // Verify tasks are actually Ready in the repo
    for task_id_str in &result.created_task_ids {
        let task_id = ralphx_lib::domain::entities::TaskId::from_string(task_id_str.clone());
        let task = state
            .task_repo
            .get_by_id(&task_id)
            .await
            .expect("repo error")
            .expect("task should exist");
        assert_eq!(
            task.internal_status,
            InternalStatus::Ready,
            "Task without blockers should be Ready"
        );
        assert_eq!(task.ideation_session_id, Some(session.id.clone()));
    }
}

#[tokio::test]
async fn test_apply_proposals_core_snapshots_blueprint_lineage_on_created_task() {
    use ralphx_lib::domain::entities::{Artifact, ArtifactType};

    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 1).await;
    let blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Blueprint",
            ArtifactType::Specification,
            "Implementation details",
            "test",
        ))
        .await
        .expect("blueprint should be created");
    let proposal_id = TaskProposalId::from_string(proposal_ids[0].clone());
    let mut proposal = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .expect("proposal lookup should succeed")
        .expect("proposal should exist");
    proposal.blueprint_artifact_id = Some(blueprint.id.clone());
    proposal.blueprint_version_at_creation = Some(1);
    state
        .task_proposal_repo
        .update(&proposal)
        .await
        .expect("proposal blueprint lineage should persist");

    let result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.to_string(),
            proposal_ids,
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("proposal acceptance should succeed");
    let task = state
        .task_repo
        .get_by_id(&ralphx_lib::domain::entities::TaskId::from_string(
            result.created_task_ids[0].clone(),
        ))
        .await
        .expect("task lookup should succeed")
        .expect("task should exist");

    assert_eq!(task.plan_blueprint_artifact_id, Some(blueprint.id));
}

#[tokio::test]
async fn test_apply_proposals_core_rejects_incomplete_blueprint_lineage() {
    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 1).await;
    let proposal_id = TaskProposalId::from_string(proposal_ids[0].clone());
    let mut proposal = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .expect("proposal lookup should succeed")
        .expect("proposal should exist");
    proposal.blueprint_version_at_creation = Some(1);
    state
        .task_proposal_repo
        .update(&proposal)
        .await
        .expect("incomplete lineage fixture should persist");

    let error = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.to_string(),
            proposal_ids,
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect_err("v2 lineage without a Blueprint ID must fail closed");

    assert!(error
        .to_string()
        .contains("blueprint lineage is incomplete"));
    assert!(
        state
            .task_repo
            .get_by_ideation_session(&session.id)
            .await
            .expect("task lookup should succeed")
            .is_empty(),
        "failed acceptance must roll back task creation"
    );
}

#[tokio::test]
async fn test_apply_proposals_core_session_converts_to_accepted() {
    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 1).await;

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed");

    assert!(
        result.session_converted,
        "All proposals applied — session should convert"
    );

    // Verify session status is Accepted in repo
    let updated_session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("repo error")
        .expect("session should exist");
    assert_eq!(updated_session.status, IdeationSessionStatus::Accepted);
}

#[tokio::test]
async fn test_restart_ideation_implementation_core_rebuilds_current_attempt_only() {
    use std::collections::HashSet;

    use ralphx_lib::domain::entities::{ExecutionPlan, ExecutionPlanId, ExecutionPlanStatus, Task};

    let state = setup_apply_test_state();
    let (project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to acknowledge dependencies");

    let apply_result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: proposal_ids.clone(),
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("apply_proposals_core should accept the session");
    let old_execution_plan_id = apply_result
        .execution_plan_id
        .clone()
        .expect("accepted session should have an execution plan");
    let old_execution_plan = ExecutionPlanId::from_string(old_execution_plan_id.clone());

    state
        .active_plan_repo
        .set(&project_id, &session.id)
        .await
        .expect("accepted session should become active plan");
    state
        .active_plan_repo
        .set_execution_plan_id(&project_id, &old_execution_plan)
        .await
        .expect("active plan should track old execution plan");

    let historical_plan = state
        .execution_plan_repo
        .create(ExecutionPlan::new(session.id.clone()))
        .await
        .expect("historical execution plan should be created");
    state
        .execution_plan_repo
        .mark_superseded(&historical_plan.id)
        .await
        .expect("historical execution plan should be superseded");
    let mut historical_task = Task::new(project_id.clone(), "Historical task".to_string());
    historical_task.ideation_session_id = Some(session.id.clone());
    historical_task.execution_plan_id = Some(historical_plan.id.clone());
    let historical_task = state
        .task_repo
        .create(historical_task)
        .await
        .expect("historical task should be created");

    let old_attempt_task_count = state
        .task_repo
        .count_tasks(&project_id, false, None, Some(old_execution_plan.as_str()))
        .await
        .expect("old attempt count should load");
    assert!(
        old_attempt_task_count > proposal_ids.len() as u32,
        "old attempt count should include the merge task"
    );

    let result = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect("restart should rebuild implementation attempt");

    assert_eq!(result.old_execution_plan_id, old_execution_plan_id);
    assert_ne!(result.execution_plan_id, old_execution_plan_id);
    assert_eq!(
        result.archived_task_count, old_attempt_task_count as usize,
        "restart should archive exactly the current active attempt"
    );
    assert_eq!(
        result.created_task_ids.len(),
        proposal_ids.len(),
        "restart should recreate one task per local proposal"
    );

    let old_plan = state
        .execution_plan_repo
        .get_by_id(&old_execution_plan)
        .await
        .expect("old plan lookup should succeed")
        .expect("old plan should exist");
    assert_eq!(old_plan.status, ExecutionPlanStatus::Superseded);

    let new_execution_plan = ExecutionPlanId::from_string(result.execution_plan_id.clone());
    let new_plan = state
        .execution_plan_repo
        .get_by_id(&new_execution_plan)
        .await
        .expect("new plan lookup should succeed")
        .expect("new plan should exist");
    assert_eq!(new_plan.status, ExecutionPlanStatus::Active);

    let old_attempt_tasks = state
        .task_repo
        .list_paginated(
            &project_id,
            None,
            0,
            old_attempt_task_count,
            true,
            None,
            Some(old_execution_plan.as_str()),
            None,
        )
        .await
        .expect("old attempt tasks should load");
    assert!(
        old_attempt_tasks
            .iter()
            .all(|task| task.archived_at.is_some()),
        "all old active-attempt tasks should be archived"
    );

    let historical_task_after_restart = state
        .task_repo
        .get_by_id(&historical_task.id)
        .await
        .expect("historical task lookup should succeed")
        .expect("historical task should still exist");
    assert!(
        historical_task_after_restart.archived_at.is_none(),
        "restart must not archive superseded historical attempts"
    );

    for task_id in &result.created_task_ids {
        let task = state
            .task_repo
            .get_by_id(&ralphx_lib::domain::entities::TaskId::from_string(
                task_id.clone(),
            ))
            .await
            .expect("new task lookup should succeed")
            .expect("new task should exist");
        assert_eq!(task.execution_plan_id, Some(new_execution_plan.clone()));
        assert!(task.archived_at.is_none());
    }

    let proposal_task_ids: HashSet<String> = state
        .task_proposal_repo
        .get_by_session(&session.id)
        .await
        .expect("proposals should load")
        .into_iter()
        .map(|proposal| {
            proposal
                .created_task_id
                .expect("proposal should point to recreated task")
                .as_str()
                .to_string()
        })
        .collect();
    let created_task_ids: HashSet<String> = result.created_task_ids.iter().cloned().collect();
    assert_eq!(proposal_task_ids, created_task_ids);

    assert_eq!(
        state
            .active_plan_repo
            .get(&project_id)
            .await
            .expect("active plan should load"),
        Some(session.id.clone())
    );
    assert_eq!(
        state
            .active_plan_repo
            .get_execution_plan_id(&project_id)
            .await
            .expect("active execution plan should load"),
        Some(new_execution_plan)
    );

    let updated_session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should exist");
    assert_eq!(updated_session.status, IdeationSessionStatus::Accepted);
}

#[tokio::test]
async fn test_restart_move_worktree_relocates_owned_workspace_before_resetting() {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, resolve_linked_plan_branch_agent_worktree_path,
        AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::application::GitService;
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus, ChatConversation,
        IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationAnalysisWorkspaceKind,
        IdeationSession, PlanBranchStatus, Priority, Project, ProposalCategory, TaskProposal,
    };

    let mut state = setup_apply_test_state();
    let github = std::sync::Arc::new(crate::support::mock_github_service::MockGithubService::new());
    github.will_return_status(
        ralphx_lib::domain::services::github_service::PrStatus::Merged {
            merge_commit_sha: Some("merged-before-second-restart".to_string()),
            merged_at: Some("2026-07-18T10:00:00Z".to_string()),
        },
    );
    state.github_service = Some(
        github
            as std::sync::Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>,
    );
    let repo_dir = setup_git_repo_for_apply_test();
    let origin_dir = tempfile::TempDir::new().unwrap();
    git_ok(
        repo_dir.path(),
        &[
            "init",
            "--bare",
            origin_dir
                .path()
                .to_str()
                .expect("origin path should be utf-8"),
        ],
    );
    git_ok(
        repo_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            origin_dir
                .path()
                .to_str()
                .expect("origin path should be utf-8"),
        ],
    );
    git_ok(repo_dir.path(), &["push", "-u", "origin", "main"]);
    let worktree_parent = tempfile::TempDir::new().unwrap();
    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create project conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent conversation workspace");

    let mut session = IdeationSession::new(project.id.clone());
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace.worktree_path.clone()),
        base_commit: workspace.base_commit.clone(),
        base_locked_at: Some(chrono::Utc::now()),
    };
    let session = state.ideation_session_repo.create(session).await.unwrap();
    workspace.linked_ideation_session_id = Some(session.id.clone());
    let workspace = state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();

    let apply_result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("apply should accept linked workspace session");
    let old_execution_plan_id = apply_result
        .execution_plan_id
        .expect("accepted session should have an execution plan");

    let plan_branch = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .unwrap()
        .expect("plan branch should exist");
    let linked_worktree_path =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("linked plan branch path should resolve");
    assert!(
        std::path::Path::new(&workspace.worktree_path).is_dir(),
        "the owned conversation worktree should hold the plan branch before restart"
    );
    assert_eq!(
        git_stdout(
            std::path::Path::new(&workspace.worktree_path),
            &["rev-parse", "--abbrev-ref", "HEAD"]
        ),
        plan_branch.branch_name
    );
    git_ok(
        std::path::Path::new(&workspace.worktree_path),
        &["checkout", "-b", "restart-branch-mismatch"],
    );
    let error = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect_err("restart should reject an owned workspace on a different branch");
    assert!(error.to_string().contains("could not safely restore"));
    assert!(
        !error.to_string().contains(&workspace.worktree_path),
        "user-facing restart errors must not expose the workspace path"
    );
    assert_eq!(
        state
            .execution_plan_repo
            .get_active_for_session(&session.id)
            .await
            .expect("active plan should remain available after rejected restart")
            .expect("active plan should remain available after rejected restart")
            .id
            .as_str(),
        old_execution_plan_id.as_str(),
        "restart must reject the mismatched worktree before replacing the active attempt"
    );
    assert!(
        std::path::Path::new(&workspace.worktree_path).is_dir(),
        "restart must not relocate the conversation worktree when its branch is mismatched"
    );
    git_ok(
        std::path::Path::new(&workspace.worktree_path),
        &["checkout", plan_branch.branch_name.as_str()],
    );
    GitService::delete_worktree(
        repo_dir.path(),
        std::path::Path::new(&workspace.worktree_path),
    )
    .await
    .expect("cleanup should remove the direct worktree while preserving its branch");
    assert!(
        GitService::branch_exists(repo_dir.path(), &plan_branch.branch_name)
            .await
            .expect("branch probe should succeed"),
        "cleanup variant should preserve the implementation branch"
    );
    git_ok(
        repo_dir.path(),
        &["commit", "--allow-empty", "-m", "advance origin base"],
    );
    git_ok(repo_dir.path(), &["push", "origin", "main"]);
    let latest_origin_base = git_stdout(repo_dir.path(), &["rev-parse", "origin/main"]);

    let result = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect("restart should use linked plan branch worktree, not stale conversation path");

    assert_eq!(result.old_execution_plan_id, old_execution_plan_id);
    assert_ne!(result.execution_plan_id, old_execution_plan_id);
    assert_eq!(result.created_task_ids.len(), 1);

    let stored_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(
        stored_workspace.worktree_path, workspace.worktree_path,
        "restart should not rewrite the stale conversation worktree path"
    );
    assert_eq!(
        stored_workspace.linked_plan_branch_id.as_ref(),
        Some(&plan_branch.id)
    );
    assert!(
        !std::path::Path::new(&workspace.worktree_path).is_dir(),
        "restart should relocate the owned conversation worktree to the linked plan path"
    );
    assert!(
        linked_worktree_path.is_dir(),
        "restart should preserve the linked plan worktree"
    );
    assert_eq!(
        git_stdout(&linked_worktree_path, &["rev-parse", "HEAD"]),
        latest_origin_base,
        "restart should reset the linked branch to the newest origin base revision"
    );

    GitService::delete_worktree(repo_dir.path(), &linked_worktree_path)
        .await
        .expect("merged cleanup should remove the linked worktree");
    git_ok(
        repo_dir.path(),
        &["branch", "-D", plan_branch.branch_name.as_str()],
    );
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &conversation.id,
            Some(90),
            Some("https://github.com/example/repo/pull/90"),
            Some("merged"),
            Some("pushed"),
        )
        .await
        .expect("test should record merged publication state");
    state
        .agent_conversation_workspace_repo
        .mark_local_cleanup_status(&conversation.id, "cleaned", chrono::Utc::now())
        .await
        .expect("test should record owned merged cleanup");
    state
        .plan_branch_repo
        .set_merged(&plan_branch.id)
        .await
        .expect("test should record the merged plan branch");
    state
        .plan_branch_repo
        .mark_local_cleanup_status(&plan_branch.id, "cleaned", chrono::Utc::now())
        .await
        .expect("test should record plan branch cleanup provenance");

    let second_restart =
        restart_ideation_implementation_core(&state, session.id.as_str().to_string())
            .await
            .expect("restart should recreate a branch removed by owned merged cleanup");
    assert_eq!(
        second_restart.old_execution_plan_id,
        result.execution_plan_id
    );
    assert!(linked_worktree_path.is_dir());
    let restored_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should remain persisted");
    assert_eq!(
        restored_workspace.status,
        AgentConversationWorkspaceStatus::Active
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_local_cleanup_status(&conversation.id)
            .await
            .expect("cleanup status should load"),
        None
    );
    let restored_plan_branch = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("restarted plan branch should remain persisted");
    assert_eq!(restored_plan_branch.status, PlanBranchStatus::Active);
    assert_eq!(
        restored_plan_branch.merged_at, None,
        "restart must clear the stale merged timestamp"
    );
    assert_eq!(
        state
            .plan_branch_repo
            .get_local_cleanup_status(&plan_branch.id)
            .await
            .expect("plan branch cleanup status should load"),
        None
    );
}

#[tokio::test]
async fn test_restart_ideation_implementation_core_requires_origin_base_before_archiving_tasks() {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, ExecutionPlanStatus,
        IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationAnalysisWorkspaceKind,
        IdeationSession, Priority, Project, ProposalCategory, TaskProposal,
    };
    use ralphx_lib::error::AppError;

    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();
    let mut project = Project::new(
        "Restart base failure test".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create project conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent conversation workspace");
    let mut session = IdeationSession::new(project.id.clone());
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace.worktree_path.clone()),
        base_commit: workspace.base_commit.clone(),
        base_locked_at: Some(chrono::Utc::now()),
    };
    let session = state.ideation_session_repo.create(session).await.unwrap();
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");
    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Restart proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("proposal should be created");
    let apply_result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("apply should create the original implementation attempt");
    let old_execution_plan_id = apply_result
        .execution_plan_id
        .expect("accepted session should have an execution plan");
    let old_task_count = state
        .task_repo
        .count_tasks(&project.id, false, None, Some(&old_execution_plan_id))
        .await
        .expect("old task count should load");

    let error = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect_err("restart should require an origin base ref");

    assert!(
        matches!(error, AppError::Validation(_) | AppError::GitOperation(_)),
        "restart should stop at strict origin-base preflight, got {error:?}"
    );
    let old_plan = state
        .execution_plan_repo
        .get_by_id(&ralphx_lib::domain::entities::ExecutionPlanId::from_string(
            old_execution_plan_id.clone(),
        ))
        .await
        .expect("old execution plan should load")
        .expect("old execution plan should remain");
    assert_eq!(old_plan.status, ExecutionPlanStatus::Active);
    assert_eq!(
        state
            .task_repo
            .count_tasks(&project.id, false, None, Some(&old_execution_plan_id))
            .await
            .expect("old task count should still load"),
        old_task_count,
        "restart must not archive tasks when base preflight fails"
    );
}

#[tokio::test]
async fn test_restart_ideation_implementation_core_rejects_linked_workspace_without_plan_branch() {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind,
        IdeationAnalysisState, IdeationAnalysisWorkspaceKind, IdeationSession, Priority, Project,
        ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();
    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create project conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent conversation workspace");

    let mut session = IdeationSession::new(project.id.clone());
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace.worktree_path.clone()),
        base_commit: workspace.base_commit.clone(),
        base_locked_at: Some(chrono::Utc::now()),
    };
    let session = state.ideation_session_repo.create(session).await.unwrap();
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();

    let apply_result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("apply should accept linked workspace session");
    let old_execution_plan_id = ralphx_lib::domain::entities::ExecutionPlanId::from_string(
        apply_result
            .execution_plan_id
            .expect("accepted session should have an execution plan"),
    );

    state
        .agent_conversation_workspace_repo
        .update_links(&conversation.id, Some(&session.id), None)
        .await
        .expect("test should remove stale plan branch link");

    let err = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect_err("restart should require a linked plan branch");

    assert!(
        err.to_string().contains("linked plan branch"),
        "error should mention the missing linked plan branch: {}",
        err
    );
    let old_plan = state
        .execution_plan_repo
        .get_by_id(&old_execution_plan_id)
        .await
        .expect("old plan lookup should succeed")
        .expect("old plan should remain");
    assert_eq!(
        old_plan.status.to_db_string(),
        "active",
        "restart should fail before mutating the active execution plan"
    );
}

#[tokio::test]
async fn test_restart_ideation_implementation_core_rejects_missing_linked_plan_branch_row() {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind,
        IdeationAnalysisState, IdeationAnalysisWorkspaceKind, IdeationSession, Priority, Project,
        ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();
    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create project conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent conversation workspace");

    let mut session = IdeationSession::new(project.id.clone());
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace.worktree_path.clone()),
        base_commit: workspace.base_commit.clone(),
        base_locked_at: Some(chrono::Utc::now()),
    };
    let session = state.ideation_session_repo.create(session).await.unwrap();
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();

    let apply_result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("apply should accept linked workspace session");
    let old_execution_plan_id = ralphx_lib::domain::entities::ExecutionPlanId::from_string(
        apply_result
            .execution_plan_id
            .expect("accepted session should have an execution plan"),
    );
    let plan_branch = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .unwrap()
        .expect("plan branch should exist before deletion");
    state
        .plan_branch_repo
        .delete(&plan_branch.id)
        .await
        .expect("test should remove linked plan branch row");

    let err = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect_err("restart should require the linked plan branch row");

    assert!(
        err.to_string().contains("Linked plan branch not found"),
        "error should mention the missing linked plan branch row: {}",
        err
    );
    let old_plan = state
        .execution_plan_repo
        .get_by_id(&old_execution_plan_id)
        .await
        .expect("old plan lookup should succeed")
        .expect("old plan should remain");
    assert_eq!(
        old_plan.status.to_db_string(),
        "active",
        "restart should fail before mutating the active execution plan"
    );
}

#[tokio::test]
async fn test_restart_ideation_implementation_core_rejects_active_session() {
    let state = setup_apply_test_state();
    let (_project_id, session, _proposal_ids) = setup_session_with_proposals(&state, 1).await;

    let err = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect_err("restart should reject active sessions");

    assert!(
        err.to_string().contains("accepted"),
        "error should require an accepted session: {}",
        err
    );
}

#[tokio::test]
async fn test_restart_ideation_implementation_core_rejects_accepted_without_active_plan() {
    let state = setup_apply_test_state();
    let (_project_id, session, _proposal_ids) = setup_session_with_proposals(&state, 1).await;

    state
        .ideation_session_repo
        .update_status(&session.id, IdeationSessionStatus::Accepted)
        .await
        .expect("session should become accepted");

    let err = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect_err("restart should require an active execution plan");

    assert!(
        err.to_string().contains("active implementation attempt"),
        "error should mention missing active implementation attempt: {}",
        err
    );
}

#[test]
fn test_restart_ideation_implementation_core_response_conversion_preserves_public_fields() {
    let response: RestartImplementationResultResponse = RestartImplementationResult {
        session_id: "session-1".to_string(),
        project_id: "project-1".to_string(),
        old_execution_plan_id: "exec-old".to_string(),
        execution_plan_id: "exec-new".to_string(),
        archived_task_count: 2,
        created_task_ids: vec!["task-1".to_string(), "task-2".to_string()],
        any_ready_tasks: true,
    }
    .into();

    assert_eq!(response.session_id, "session-1");
    assert_eq!(response.old_execution_plan_id, "exec-old");
    assert_eq!(response.execution_plan_id, "exec-new");
    assert_eq!(response.archived_task_count, 2);
    assert_eq!(response.created_task_ids, ["task-1", "task-2"]);
}

#[tokio::test]
async fn test_apply_proposals_core_partial_apply_does_not_convert_session() {
    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    // Only apply 1 of 2 proposals
    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![proposal_ids[0].clone()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed");

    assert!(
        !result.session_converted,
        "Partial apply should not convert session"
    );

    // Session should still be Active
    let updated_session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("repo error")
        .expect("session should exist");
    assert_eq!(updated_session.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_apply_proposals_core_idempotency_guard() {
    use ralphx_lib::domain::entities::{ExecutionPlan, Task};

    let state = setup_apply_test_state();
    let (project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    // Pre-seed an active ExecutionPlan for this session to simulate a race condition
    // (two simultaneous accepts before either updates the session status).
    // Also create a task linked to the plan so the orphan-supersede guard does NOT fire —
    // the idempotency guard only activates when there are existing tasks or applied proposals.
    let existing_plan = ExecutionPlan::new(session.id.clone());
    let created_plan = state
        .execution_plan_repo
        .create(existing_plan)
        .await
        .expect("Failed to create pre-existing execution plan");

    // Create a real task tied to this plan so the orphan check sees it as non-orphan
    let mut stub_task = Task::new(project_id.clone(), "stub task".to_string());
    stub_task.ideation_session_id = Some(session.id.clone());
    stub_task.execution_plan_id = Some(created_plan.id.clone());
    state
        .task_repo
        .create(stub_task)
        .await
        .expect("Failed to create stub task for idempotency test");

    // Apply should hit the idempotency guard and return early
    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("Idempotency guard should return Ok, not error");

    assert_eq!(
        result.created_task_ids.len(),
        0,
        "Idempotency guard: no tasks created when plan already exists"
    );
    assert!(
        !result.warnings.is_empty(),
        "Idempotency guard: should emit a warning"
    );
    assert!(
        result.warnings[0].contains("already active"),
        "Warning should mention existing plan"
    );
}

#[tokio::test]
async fn test_apply_proposals_core_repairs_stale_orphaned_execution_plan() {
    use ralphx_lib::domain::entities::{ExecutionPlan, ExecutionPlanStatus, IdeationSessionStatus};

    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    // Orphan plan: 0 tasks, 0 applied proposals — superseded immediately regardless of age.
    let orphan_plan = ExecutionPlan::new(session.id.clone());
    let stale_plan_id = orphan_plan.id.clone();
    state
        .execution_plan_repo
        .create(orphan_plan)
        .await
        .expect("Failed to create orphan execution plan");

    // Acknowledge dependencies (required by gate for multi-proposal sessions)
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to set dependencies_acknowledged");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("Retry should supersede orphan execution plan and succeed");

    assert_eq!(
        result.created_task_ids.len(),
        2,
        "Retry should create tasks"
    );
    assert!(result.session_converted, "Retry should convert the session");

    let orphan_plan = state
        .execution_plan_repo
        .get_by_id(&stale_plan_id)
        .await
        .expect("repo error")
        .expect("orphan plan should still exist");
    assert_eq!(
        orphan_plan.status,
        ExecutionPlanStatus::Superseded,
        "orphan execution plan should be superseded"
    );

    let updated_session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("repo error")
        .expect("session should exist");
    assert_eq!(updated_session.status, IdeationSessionStatus::Accepted);
}

#[tokio::test]
async fn test_apply_proposals_core_ignores_retired_active_verification_when_accept_gate_disabled() {
    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 1).await;

    state
        .ideation_session_repo
        .update_verification_state(&session.id, VerificationStatus::Reviewing, true)
        .await
        .expect("Failed to mark verification active");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("retired verifier state must not block advisory acceptance");
    assert_eq!(result.tasks_created, 1);

    let active_plan = state
        .execution_plan_repo
        .get_active_for_session(&session.id)
        .await
        .expect("execution plan lookup should not fail");
    assert!(active_plan.is_some());

    let tasks = state
        .task_repo
        .get_by_ideation_session(&session.id)
        .await
        .expect("task lookup should not fail");
    assert!(!tasks.is_empty());

    let updated_session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("repo error")
        .expect("session should exist");
    assert_eq!(updated_session.status, IdeationSessionStatus::Accepted);
}

#[tokio::test]
async fn test_apply_proposals_core_accepts_only_current_exact_verification_proof() {
    use ralphx_lib::domain::entities::{Artifact, ArtifactType};

    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 1).await;
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Overview",
            ArtifactType::Specification,
            "# Overview",
            "test",
        ))
        .await
        .expect("overview should be created");
    let blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Blueprint",
            ArtifactType::Specification,
            "# Blueprint",
            "test",
        ))
        .await
        .expect("blueprint should be created");

    let session_id = session.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET plan_artifact_id = ?1,
                     plan_blueprint_artifact_id = ?2,
                     verified_plan_artifact_id = ?1,
                     verified_plan_blueprint_artifact_id = ?2
                 WHERE id = ?3",
                rusqlite::params![overview.id.as_str(), blueprint.id.as_str(), session_id],
            )?;
            Ok(())
        })
        .await
        .expect("exact proof should be stored");

    let mut settings = state
        .ideation_settings_repo
        .get_settings()
        .await
        .expect("settings should load");
    settings.require_verification_for_accept = true;
    settings.auto_verify_plans = false;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .expect("settings should update");

    let result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids,
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("exact proof for the current artifact should permit acceptance");

    assert_eq!(result.tasks_created, 1);
    assert!(result.session_converted);
}

#[tokio::test]
async fn test_apply_proposals_core_blocks_stale_proof_without_creating_kanban_state() {
    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 1).await;

    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-current".to_string()))
        .await
        .expect("plan should be linked");
    let session_id = session.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET verified_plan_artifact_id = 'plan-stale'
                 WHERE id = ?1",
                [&session_id],
            )?;
            Ok(())
        })
        .await
        .expect("stale proof should be stored");

    let mut settings = state
        .ideation_settings_repo
        .get_settings()
        .await
        .expect("settings should load");
    settings.require_verification_for_accept = true;
    settings.auto_verify_plans = false;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .expect("settings should update");

    let error = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids,
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect_err("stale proof must block acceptance");

    assert!(error.to_string().contains("must be verified"));
    assert!(
        state
            .execution_plan_repo
            .get_active_for_session(&session.id)
            .await
            .expect("execution plan lookup should succeed")
            .is_none(),
        "blocked acceptance must not create an execution plan"
    );
    assert!(
        state
            .task_repo
            .get_by_ideation_session(&session.id)
            .await
            .expect("task lookup should succeed")
            .is_empty(),
        "blocked acceptance must not create tasks"
    );
}

#[tokio::test]
async fn test_apply_proposals_core_blocks_unverified_all_foreign_acceptance() {
    use ralphx_lib::domain::entities::{
        IdeationSession, Priority, Project, ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let project = state
        .project_repo
        .create(Project::new(
            "Foreign-only source".to_string(),
            "/tmp/foreign-only-source".to_string(),
        ))
        .await
        .expect("project should be created");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should be created");
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-current".to_string()))
        .await
        .expect("plan should be linked");
    let mut proposal = TaskProposal::new(
        session.id.clone(),
        "Foreign proposal".to_string(),
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.target_project = Some("/tmp/another-project".to_string());
    let proposal = state
        .task_proposal_repo
        .create(proposal)
        .await
        .expect("proposal should be created");

    let mut settings = state
        .ideation_settings_repo
        .get_settings()
        .await
        .expect("settings should load");
    settings.require_verification_for_accept = true;
    settings.auto_verify_plans = false;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .expect("settings should update");

    let error = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect_err("required verification must also gate all-foreign acceptance");

    assert!(error.to_string().contains("must be verified"));
    let persisted = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session read should succeed")
        .expect("session should exist");
    assert_eq!(persisted.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_apply_proposals_core_rejects_inactive_session() {
    use ralphx_lib::error::AppError;

    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 1).await;

    // Archive the session so it is no longer Active
    state
        .ideation_session_repo
        .update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .expect("Failed to archive session");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let err = apply_proposals_core(&state, input)
        .await
        .expect_err("Should fail for inactive session");

    assert!(
        matches!(err, AppError::Validation(_)),
        "Expected Validation error, got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_apply_proposals_core_rejects_unknown_proposals() {
    use ralphx_lib::error::AppError;

    let state = setup_apply_test_state();
    let (_project_id, session, _) = setup_session_with_proposals(&state, 1).await;

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec!["nonexistent-proposal-id".to_string()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let err = apply_proposals_core(&state, input)
        .await
        .expect_err("Should fail when proposals not found");

    assert!(
        matches!(err, AppError::Validation(_)),
        "Expected Validation error, got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_apply_proposals_core_result_contains_context_fields() {
    let state = setup_apply_test_state();
    let (project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    // Acknowledge dependencies (required by gate for multi-proposal sessions)
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to set dependencies_acknowledged");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed");

    // Verify context fields for Tauri side effects
    assert_eq!(
        result.project_id,
        project_id.as_str(),
        "project_id must match"
    );
    assert_eq!(
        result.session_id,
        session.id.as_str(),
        "session_id must match"
    );
    assert_eq!(
        result.proposal_titles.len(),
        2,
        "proposal_titles should contain all applied titles"
    );
    assert!(!result.is_user_title, "New session has no user title");
    assert!(
        result.execution_plan_id.is_some(),
        "execution_plan_id must be set"
    );
}

#[tokio::test]
async fn test_apply_proposals_core_preserves_dependencies() {
    use ralphx_lib::domain::entities::{InternalStatus, Priority, Project, ProposalCategory};

    let state = setup_apply_test_state();

    let project = Project::new("Dep Test".to_string(), "/tmp/dep".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Create p1 (blocker) and p2 (depends on p1)
    let p1 = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Blocker Task",
            ProposalCategory::Feature,
            Priority::High,
        ))
        .await
        .expect("Failed to create p1");

    let p2 = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Dependent Task",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("Failed to create p2");

    // p2 depends on p1
    state
        .proposal_dependency_repo
        .add_dependency(&p2.id, &p1.id, None, Some("manual"))
        .await
        .expect("Failed to add proposal dependency");

    // Acknowledge dependencies (deps were set, simulating agent having used dep tools)
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to set dependencies_acknowledged");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![p1.id.as_str().to_string(), p2.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed");

    assert_eq!(result.created_task_ids.len(), 2);
    assert_eq!(
        result.dependencies_created, 1,
        "One dependency should be created"
    );

    // Verify statuses: p1 task → Ready, p2 task → Blocked
    let tasks = state
        .task_repo
        .get_by_project(&project.id)
        .await
        .expect("Failed to get tasks");

    let blocker_task = tasks
        .iter()
        .find(|t| t.title == "Blocker Task")
        .expect("Blocker task not found");
    let dependent_task = tasks
        .iter()
        .find(|t| t.title == "Dependent Task")
        .expect("Dependent task not found");

    assert_eq!(
        blocker_task.internal_status,
        InternalStatus::Ready,
        "Blocker task has no blockers → Ready"
    );
    assert_eq!(
        dependent_task.internal_status,
        InternalStatus::Blocked,
        "Dependent task has blocker → Blocked"
    );
    assert!(
        dependent_task.blocked_reason.is_some(),
        "Blocked task should have a reason"
    );
}

// ============================================================================
// create_ideation_session event emission tests
// ============================================================================

#[tokio::test]
async fn test_create_ideation_session_emits_session_created_event() {
    use ralphx_lib::testing::create_mock_app;
    use std::sync::{Arc, Mutex};
    use tauri::Listener;

    let app = create_mock_app();
    let handle = app.handle().clone();
    let state = setup_apply_test_state();

    let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let captured_clone = Arc::clone(&captured);

    handle.listen("ideation:session_created", move |event| {
        let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap_or_default();
        *captured_clone.lock().unwrap() = Some(payload);
    });

    let project_id = ProjectId::new();
    let mut project = Project::new("Test Project".to_string(), "/tmp/test".to_string());
    project.id = project_id.clone();
    state.project_repo.create(project).await.unwrap();
    let input = CreateSessionInput {
        project_id: project_id.to_string(),
        title: Some("Test Event Session".to_string()),
        seed_task_id: None,
        analysis_base_ref_kind: None,
        analysis_base_ref: None,
        analysis_base_display_name: None,
    };

    let result = create_ideation_session_impl(&handle, &state, input)
        .await
        .expect("create_ideation_session_impl should succeed");

    assert_eq!(result.project_id, project_id.to_string());
    assert_eq!(result.title, Some("Test Event Session".to_string()));

    let payload = captured.lock().unwrap().clone();
    assert!(
        payload.is_some(),
        "ideation:session_created event should have been emitted"
    );
    let payload = payload.unwrap();
    assert_eq!(
        payload["projectId"].as_str().unwrap(),
        project_id.to_string(),
        "event payload projectId should match"
    );
    assert!(
        payload["sessionId"].is_string(),
        "event payload sessionId should be a string"
    );
    assert_eq!(
        payload["sessionId"].as_str().unwrap(),
        result.id,
        "event payload sessionId should match created session id"
    );
}

#[tokio::test]
async fn test_create_ideation_session_local_branch_provisions_worktree() {
    use ralphx_lib::domain::entities::{
        IdeationAnalysisBaseRefKind, IdeationAnalysisWorkspaceKind,
    };
    use ralphx_lib::testing::create_mock_app;
    use std::path::PathBuf;

    let app = create_mock_app();
    let handle = app.handle().clone();
    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();

    git_ok(repo_dir.path(), &["branch", "feature/analysis"]);

    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let result = create_ideation_session_impl(
        &handle,
        &state,
        CreateSessionInput {
            project_id: project.id.to_string(),
            title: Some("Branch Session".to_string()),
            seed_task_id: None,
            analysis_base_ref_kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            analysis_base_ref: Some("feature/analysis".to_string()),
            analysis_base_display_name: Some("feature/analysis".to_string()),
        },
    )
    .await
    .expect("create session should provision ideation worktree");

    assert_eq!(
        result.analysis_base_ref.as_deref(),
        Some("feature/analysis")
    );
    assert_eq!(result.analysis_workspace_kind, "ideation_worktree");

    let workspace = PathBuf::from(
        result
            .analysis_workspace_path
            .as_deref()
            .expect("workspace path should be persisted"),
    );
    assert!(workspace.is_dir(), "workspace should exist");
    assert_ne!(workspace, repo_dir.path());

    assert_eq!(
        git_stdout(&workspace, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feature/analysis"
    );

    let stored = state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(result.id))
        .await
        .expect("load stored ideation session")
        .expect("stored ideation session should exist");
    assert_eq!(
        stored.analysis.workspace_kind,
        IdeationAnalysisWorkspaceKind::IdeationWorktree
    );
    assert_eq!(
        stored.analysis.workspace_path.as_deref(),
        workspace.to_str()
    );
}

#[tokio::test]
async fn test_create_ideation_session_project_default_locks_main_when_current_branch_differs() {
    use ralphx_lib::domain::entities::{
        IdeationAnalysisBaseRefKind, IdeationAnalysisWorkspaceKind,
    };
    use ralphx_lib::testing::create_mock_app;
    use std::path::PathBuf;

    let app = create_mock_app();
    let handle = app.handle().clone();
    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();

    let main_sha = git_stdout(repo_dir.path(), &["rev-parse", "main"]);
    git_ok(repo_dir.path(), &["checkout", "-b", "pr/366"]);
    git_ok(
        repo_dir.path(),
        &["commit", "--allow-empty", "-m", "pr branch commit"],
    );
    let pr_sha = git_stdout(repo_dir.path(), &["rev-parse", "HEAD"]);
    assert_ne!(main_sha, pr_sha, "test requires divergent current branch");

    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let result = create_ideation_session_impl(
        &handle,
        &state,
        CreateSessionInput {
            project_id: project.id.to_string(),
            title: Some("Main Base Session".to_string()),
            seed_task_id: None,
            analysis_base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            analysis_base_ref: Some("main".to_string()),
            analysis_base_display_name: Some("Project default (main)".to_string()),
        },
    )
    .await
    .expect("create session should lock the selected project default branch");

    assert_eq!(
        result.analysis_base_ref_kind.as_deref(),
        Some("project_default")
    );
    assert_eq!(result.analysis_base_ref.as_deref(), Some("main"));
    assert_eq!(
        result.analysis_base_commit.as_deref(),
        Some(main_sha.as_str())
    );
    assert_ne!(
        result.analysis_base_commit.as_deref(),
        Some(pr_sha.as_str())
    );
    assert_eq!(result.analysis_workspace_kind, "ideation_worktree");

    let workspace = PathBuf::from(
        result
            .analysis_workspace_path
            .as_deref()
            .expect("workspace path should be persisted"),
    );
    assert!(workspace.is_dir(), "workspace should exist");
    assert_ne!(workspace, repo_dir.path());
    assert_eq!(
        git_stdout(&workspace, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
    assert_eq!(git_stdout(&workspace, &["rev-parse", "HEAD"]), main_sha);

    let stored = state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(result.id))
        .await
        .expect("load stored ideation session")
        .expect("stored ideation session should exist");
    assert_eq!(
        stored.analysis.base_ref_kind,
        Some(IdeationAnalysisBaseRefKind::ProjectDefault)
    );
    assert_eq!(stored.analysis.base_ref.as_deref(), Some("main"));
    assert_eq!(
        stored.analysis.base_commit.as_deref(),
        Some(main_sha.as_str())
    );
    assert_eq!(
        stored.analysis.workspace_kind,
        IdeationAnalysisWorkspaceKind::IdeationWorktree
    );
}

#[tokio::test]
async fn test_create_ideation_session_rejects_mislabeled_project_default_ref() {
    use ralphx_lib::domain::entities::IdeationAnalysisBaseRefKind;
    use ralphx_lib::testing::create_mock_app;

    let app = create_mock_app();
    let handle = app.handle().clone();
    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();

    git_ok(repo_dir.path(), &["checkout", "-b", "pr/366"]);

    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let err = create_ideation_session_impl(
        &handle,
        &state,
        CreateSessionInput {
            project_id: project.id.to_string(),
            title: Some("Mislabeled Base Session".to_string()),
            seed_task_id: None,
            analysis_base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            analysis_base_ref: Some("pr/366".to_string()),
            analysis_base_display_name: Some("Project default (main)".to_string()),
        },
    )
    .await
    .expect_err("project_default ref must match the resolved project default");

    assert!(
        err.contains("Project default ideation base ref 'pr/366' does not match resolved project default 'main'"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_create_ideation_session_project_default_ignores_blank_ref() {
    use ralphx_lib::domain::entities::IdeationAnalysisBaseRefKind;
    use ralphx_lib::testing::create_mock_app;

    let app = create_mock_app();
    let handle = app.handle().clone();
    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();

    let main_sha = git_stdout(repo_dir.path(), &["rev-parse", "main"]);
    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let result = create_ideation_session_impl(
        &handle,
        &state,
        CreateSessionInput {
            project_id: project.id.to_string(),
            title: Some("Blank Base Ref Session".to_string()),
            seed_task_id: None,
            analysis_base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            analysis_base_ref: Some("   ".to_string()),
            analysis_base_display_name: Some("Project default (main)".to_string()),
        },
    )
    .await
    .expect("blank project_default ref should resolve to project default");

    assert_eq!(result.analysis_base_ref.as_deref(), Some("main"));
    assert_eq!(
        result.analysis_base_commit.as_deref(),
        Some(main_sha.as_str())
    );
}

// ============================================================================
// Proof Obligation #5: branch creation failure leaves no orphaned ExecutionPlan
// ============================================================================

/// Sets up a real git repo in a tempdir with an initial commit on main.
fn setup_git_repo_for_apply_test() -> tempfile::TempDir {
    use std::process::Command;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(path)
        .output()
        .unwrap();
    dir
}

fn git_ok(repo: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::test]
async fn test_apply_proposals_core_internal_session_uses_session_analysis_base() {
    use ralphx_lib::domain::entities::{
        IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationAnalysisWorkspaceKind,
        IdeationSession, Priority, Project, ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    let project = state.project_repo.create(project).await.unwrap();

    let mut session = IdeationSession::new(project.id.clone());
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::ProjectRoot,
        workspace_path: Some(repo_dir.path().to_string_lossy().to_string()),
        base_commit: None,
        base_locked_at: Some(chrono::Utc::now()),
    };
    let session = state.ideation_session_repo.create(session).await.unwrap();

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();

    apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: Some("late-override".to_string()),
        },
    )
    .await
    .expect("internal apply should derive base from session analysis");

    let plan_branch = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .unwrap()
        .expect("plan branch should exist");
    assert_eq!(plan_branch.source_branch, "main");
    assert_eq!(plan_branch.base_branch_override.as_deref(), Some("main"));
}

#[tokio::test]
async fn test_apply_proposals_core_linked_agent_workspace_reuses_conversation_branch() {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind,
        IdeationAnalysisState, IdeationAnalysisWorkspaceKind, IdeationSession, Priority, Project,
        ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let repo_dir = setup_git_repo_for_apply_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();
    let mut project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create project conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent conversation workspace");

    let mut session = IdeationSession::new(project.id.clone());
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace.worktree_path.clone()),
        base_commit: workspace.base_commit.clone(),
        base_locked_at: Some(chrono::Utc::now()),
    };
    let session = state.ideation_session_repo.create(session).await.unwrap();
    workspace.linked_ideation_session_id = Some(session.id.clone());
    let workspace = state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();

    apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("apply should reuse linked agent conversation branch");

    let plan_branch = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .unwrap()
        .expect("plan branch should exist");
    assert_eq!(plan_branch.branch_name, workspace.branch_name);
    assert_eq!(plan_branch.source_branch, "main");
    assert_eq!(plan_branch.base_branch_override.as_deref(), Some("main"));

    let linked_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(
        linked_workspace.linked_plan_branch_id.as_ref(),
        Some(&plan_branch.id)
    );
}

/// Proof Obligation #5: when `create_branch` fails (source branch nonexistent),
/// `apply_proposals_core` returns an error AND no ExecutionPlan row is created.
/// Validates that branch validation (Phase 0) occurs before ExecutionPlan creation.
#[tokio::test]
async fn test_apply_proposals_core_branch_creation_failure_leaves_no_orphaned_execution_plan() {
    use ralphx_lib::domain::entities::{
        IdeationSession, Priority, Project, ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let dir = setup_git_repo_for_apply_test();

    // Create a project pointing to the real git repo.
    // Set base_branch to a nonexistent branch so create_branch fails.
    let mut project = Project::new(
        "Test Project".to_string(),
        dir.path().to_str().unwrap().to_string(),
    );
    project.base_branch = Some("nonexistent-source".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    let proposal = TaskProposal::new(
        session.id.clone(),
        "Test Proposal".to_string(),
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let proposal = state
        .task_proposal_repo
        .create(proposal)
        .await
        .expect("Failed to create proposal");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![proposal.id.as_str().to_string()],
        target_column: "auto".to_string(),
        // "new-branch" doesn't exist; base is "nonexistent-source" → create_branch fails
        base_branch_override: Some("new-branch".to_string()),
    };

    let err = apply_proposals_core(&state, input)
        .await
        .expect_err("apply_proposals_core should fail when branch creation fails");

    assert!(
        matches!(err, ralphx_lib::error::AppError::Validation(_)),
        "Expected Validation error, got: {:?}",
        err
    );

    // Proof Obligation #5: no ExecutionPlan row should exist — branch check is Phase 0,
    // before ExecutionPlan creation.
    let active_plan = state
        .execution_plan_repo
        .get_active_for_session(&session.id)
        .await
        .expect("repo error");
    assert!(
        active_plan.is_none(),
        "No ExecutionPlan should be created when branch creation fails (Proof Obligation #5)"
    );
}

// ============================================================================
// Counter fix + dependency acknowledgment gate integration tests
// ============================================================================

/// Proof Obligation #1/#2: `tasks_created` equals the number of plan tasks (not counting merge
/// task), and `dependencies_created` is 0 when no proposal-to-proposal deps exist even when a
/// merge task edge is created (feature branch path).
#[tokio::test]
async fn test_apply_proposals_core_tasks_created_count_excludes_merge_task() {
    use ralphx_lib::domain::entities::{
        IdeationSession, Priority, Project, ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let dir = setup_git_repo_for_apply_test();

    // Project pointing to a real git repo; "main" is a valid source branch.
    let mut project = Project::new(
        "Counter Fix Test Project".to_string(),
        dir.path().to_str().unwrap().to_string(),
    );
    project.base_branch = Some("main".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Create 3 proposals (no dependencies between them)
    let mut proposal_ids = Vec::new();
    for i in 1..=3 {
        let p = state
            .task_proposal_repo
            .create(TaskProposal::new(
                session.id.clone(),
                format!("Proposal {}", i),
                ProposalCategory::Feature,
                Priority::Medium,
            ))
            .await
            .expect("Failed to create proposal");
        proposal_ids.push(p.id.as_str().to_string());
    }

    // Acknowledge dependencies (agent reviewed the graph — no proposal-to-proposal deps).
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to set dependencies_acknowledged");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: proposal_ids.clone(),
        target_column: "auto".to_string(),
        base_branch_override: Some("feature/counter-fix-test".to_string()),
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed with valid feature branch");

    // Proof Obligation #2: tasks_created counts only plan tasks, NOT the merge task.
    assert_eq!(
        result.tasks_created, 3,
        "tasks_created should equal the number of proposals (3), excluding the auto-generated merge task"
    );
    // Proof Obligation #1: merge task edge must NOT be counted in dependencies_created.
    assert_eq!(
        result.dependencies_created, 0,
        "dependencies_created should be 0 — no proposal-to-proposal deps exist (merge task edge excluded)"
    );
}

/// Proof Obligation #3: gate blocks `apply_proposals_core` for multi-proposal sessions where
/// the agent never acknowledged dependency ordering.
#[tokio::test]
async fn test_apply_proposals_core_gate_blocks_unacknowledged_session() {
    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    // Intentionally do NOT call set_dependencies_acknowledged — simulates an agent that
    // created proposals and called finalize without reviewing dependency ordering at all.

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let err = apply_proposals_core(&state, input)
        .await
        .expect_err("apply_proposals_core should be blocked by the dependency acknowledgment gate");

    assert!(
        matches!(err, ralphx_lib::error::AppError::Validation(_)),
        "Expected Validation error from dependency gate, got: {:?}",
        err
    );
    // The error message must be actionable — tell the agent exactly what tool to call.
    let msg = err.to_string();
    assert!(
        msg.contains("dependency ordering has not been reviewed"),
        "Gate error message must be actionable, got: {}",
        msg
    );
}

/// Proof Obligation #4: gate passes after `analyze_session_dependencies` is called (even with
/// 0 proposal-to-proposal deps). Calling the tool proves the agent considered the dep graph.
#[tokio::test]
async fn test_apply_proposals_core_gate_passes_after_analyze_session_dependencies() {
    let state = setup_apply_test_state();
    let (_project_id, session, proposal_ids) = setup_session_with_proposals(&state, 2).await;

    // Simulate `analyze_session_dependencies` handler: it calls set_dependencies_acknowledged
    // after computing the graph, marking that the agent reviewed dependency ordering.
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to simulate analyze_session_dependencies acknowledgment");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed after analyze_session_dependencies");

    // Gate passed; 0 proposal-to-proposal deps is valid when agent explicitly reviewed the graph.
    assert_eq!(
        result.dependencies_created, 0,
        "0 deps is acceptable when agent acknowledged via analyze_session_dependencies"
    );
    assert_eq!(result.created_task_ids.len(), 2, "Should create 2 tasks");
}

/// Proof Obligation #5: gate passes when deps were set via `create_task_proposal(depends_on:[...])`
/// at creation time. `create_proposal_impl` auto-sets `dependencies_acknowledged=true` as a side
/// effect of any non-empty `depends_on`. This test simulates that auto-set path.
#[tokio::test]
async fn test_apply_proposals_core_gate_passes_with_deps_set_at_creation() {
    use ralphx_lib::domain::entities::{
        IdeationSession, Priority, Project, ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();

    let project = Project::new("Dep At Creation Test".to_string(), "/tmp/test".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    let p1 = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Foundation Task".to_string(),
            ProposalCategory::Feature,
            Priority::High,
        ))
        .await
        .expect("Failed to create p1");

    let p2 = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Dependent Task".to_string(),
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("Failed to create p2");

    // p2 depends on p1 — set via proposal_dependency_repo as create_proposal_impl would do.
    state
        .proposal_dependency_repo
        .add_dependency(&p2.id, &p1.id, None, Some("set_at_creation"))
        .await
        .expect("Failed to add proposal dependency");

    // Auto-set by create_proposal_impl when depends_on is non-empty at creation time.
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to simulate auto-acknowledgment from create_proposal_impl");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![p1.id.as_str().to_string(), p2.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed when deps were set at creation");

    // Gate passed because deps were acknowledged via the creation path.
    assert_eq!(
        result.dependencies_created, 1,
        "One proposal-to-proposal dependency should be created"
    );
    assert_eq!(result.created_task_ids.len(), 2, "Should create 2 tasks");
}

// ============================================================================
// Finalize UPSERT, atomicity, and dedup guard tests
// ============================================================================

/// Proof: UPSERT handles a pre-existing abandoned plan_branch without a UNIQUE constraint error.
///
/// Scenario: A prior failed finalize attempt (or `enable_feature_branch`) left an abandoned
/// `plan_branch` row with `session_id = X`. When `apply_proposals_core` runs next with
/// the `ON CONFLICT(session_id) DO UPDATE` replaces the stale row
/// instead of failing with a UNIQUE constraint error.
#[tokio::test]
async fn test_finalize_with_preexisting_abandoned_plan_branch() {
    use ralphx_lib::domain::entities::{
        ArtifactId, IdeationSession, IdeationSessionStatus, PlanBranch, Priority, Project,
        ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let dir = setup_git_repo_for_apply_test();

    let mut project = Project::new(
        "UPSERT Test".to_string(),
        dir.path().to_str().unwrap().to_string(),
    );
    project.base_branch = Some("main".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("Failed to create proposal");

    // Pre-seed an abandoned plan_branch for this session (no execution_plan linked).
    // This simulates a branch created via enable_feature_branch or an abandoned prior finalize.
    // The old code (INSERT without UPSERT) would fail here with UNIQUE constraint on session_id.
    let abandoned_branch = PlanBranch::new(
        ArtifactId::from_string("old-artifact-id"),
        session.id.clone(),
        project.id.clone(),
        "ralphx/test/plan-old".to_string(),
        "main".to_string(),
    );
    let abandoned_branch = state
        .plan_branch_repo
        .create(abandoned_branch)
        .await
        .expect("Failed to pre-seed abandoned plan_branch");

    // Verify the pre-seeded branch exists with no execution_plan
    let pre_existing = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .expect("repo error")
        .expect("Pre-seeded plan_branch should exist");
    assert_eq!(pre_existing.id, abandoned_branch.id);
    assert!(
        pre_existing.execution_plan_id.is_none(),
        "Pre-seeded branch has no execution_plan"
    );

    // Call apply with feature branch — the UPSERT should update the existing row
    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![proposal.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: Some("feature/upsert-test".to_string()),
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed via UPSERT (no UNIQUE constraint error)");

    assert_eq!(result.created_task_ids.len(), 1, "Should create 1 task");
    assert!(
        result.execution_plan_id.is_some(),
        "Should return execution_plan_id"
    );
    assert!(result.session_converted, "Session should be Accepted");

    // Verify the plan_branch was UPSERTED: same session, now has new execution_plan_id
    let upserted = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .expect("repo error")
        .expect("UPSERTED plan_branch should exist");

    let expected_ep_id = result.execution_plan_id.as_deref().unwrap();
    assert_eq!(
        upserted.execution_plan_id.as_ref().map(|id| id.as_str()),
        Some(expected_ep_id),
        "UPSERTED plan_branch must have the new execution_plan_id"
    );

    // Session converted to Accepted
    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("repo error")
        .expect("session exists");
    assert_eq!(updated.status, IdeationSessionStatus::Accepted);
}

/// Proof: orphan ExecutionPlan WITH an associated PlanBranch gets cleaned up before retry.
///
/// Scenario: A prior failed finalize attempt left both:
///   - An orphan `execution_plan` row (active, 0 tasks)
///   - An associated `plan_branch` row (linked via `execution_plan_id`)
/// apply_proposals_core should detect the orphan plan (0 tasks), delete the orphan branch,
/// supersede the orphan plan, and then create a new plan + tasks successfully.
///
/// This covers the branch-deletion code path in the dedup guard (lines ~120-141 of
/// ideation_commands_apply.rs).
#[tokio::test]
async fn test_refinalize_after_orphan_plan_and_orphan_branch() {
    use ralphx_lib::domain::entities::{
        ArtifactId, ExecutionPlan, ExecutionPlanStatus, IdeationSession, IdeationSessionStatus,
        PlanBranch, Priority, Project, ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();

    let project = Project::new("Orphan Branch Test".to_string(), "/tmp/test".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("Failed to create proposal");

    // Create orphan execution plan (no tasks — simulates a failed atomic finalize)
    let orphan_plan = ExecutionPlan::new(session.id.clone());
    let orphan_plan_id = orphan_plan.id.clone();
    let orphan_plan = state
        .execution_plan_repo
        .create(orphan_plan)
        .await
        .expect("Failed to create orphan execution plan");

    // Create an orphan plan_branch associated with the orphan execution_plan
    let mut orphan_branch = PlanBranch::new(
        ArtifactId::from_string("old-artifact"),
        session.id.clone(),
        project.id.clone(),
        "ralphx/test/plan-orphan".to_string(),
        "main".to_string(),
    );
    orphan_branch.execution_plan_id = Some(orphan_plan.id.clone());
    let orphan_branch = state
        .plan_branch_repo
        .create(orphan_branch)
        .await
        .expect("Failed to create orphan plan_branch");

    // Verify pre-conditions
    let found = state
        .plan_branch_repo
        .get_by_id(&orphan_branch.id)
        .await
        .expect("repo error")
        .expect("orphan branch should exist");
    assert_eq!(found.execution_plan_id, Some(orphan_plan_id.clone()));

    // Call apply — should supersede the orphan plan + delete the orphan branch
    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![proposal.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply should succeed after orphan plan+branch cleanup");

    assert_eq!(result.created_task_ids.len(), 1, "Should create 1 task");
    assert!(result.session_converted, "Session should be Accepted");

    // Orphan plan must be superseded (not active)
    let orphan_after = state
        .execution_plan_repo
        .get_by_id(&orphan_plan_id)
        .await
        .expect("repo error")
        .expect("orphan plan should still exist in DB (superseded)");
    assert_eq!(
        orphan_after.status,
        ExecutionPlanStatus::Superseded,
        "Orphan execution_plan should be Superseded"
    );

    // Orphan branch must be deleted by the cleanup code
    let orphan_branch_after = state
        .plan_branch_repo
        .get_by_id(&orphan_branch.id)
        .await
        .expect("repo error");
    assert!(
        orphan_branch_after.is_none(),
        "Orphan plan_branch should be deleted during orphan cleanup"
    );

    // New execution plan was created (different ID from orphan)
    let new_ep_id = result.execution_plan_id.as_deref().unwrap();
    assert_ne!(
        new_ep_id,
        orphan_plan_id.as_str(),
        "New execution plan must have a different ID from the orphan"
    );

    // Session is now Accepted
    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("repo error")
        .expect("session exists");
    assert_eq!(updated.status, IdeationSessionStatus::Accepted);
}

/// Proof: the atomic transaction creates ExecutionPlan + PlanBranch + Tasks as a consistent unit.
///
/// After a successful apply_proposals_core, all three record types must exist and be linked
/// by the same `execution_plan_id`. This verifies the SUCCESS side of the atomicity guarantee:
/// no partial state (e.g., ExecutionPlan created but tasks missing) can occur.
///
/// The FAILURE side of atomicity (rollback leaves no orphans) is proven by:
///   - `test_apply_proposals_core_branch_creation_failure_leaves_no_orphaned_execution_plan`
///     (pre-transaction branch check fails → no ExecutionPlan row)
///   - `test_finalize_with_preexisting_abandoned_plan_branch` (UPSERT succeeds where old
///     INSERT would have left an orphan ExecutionPlan from a partial write sequence)
#[tokio::test]
async fn test_finalize_atomicity_all_records_consistent() {
    use ralphx_lib::domain::entities::{
        ExecutionPlanId, IdeationSession, Priority, Project, ProposalCategory, TaskProposal,
    };

    let state = setup_apply_test_state();
    let dir = setup_git_repo_for_apply_test();

    let mut project = Project::new(
        "Atomicity Test".to_string(),
        dir.path().to_str().unwrap().to_string(),
    );
    project.base_branch = Some("main".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    let p1 = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Proposal A",
            ProposalCategory::Feature,
            Priority::High,
        ))
        .await
        .expect("Failed to create p1");

    let p2 = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Proposal B",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("Failed to create p2");

    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to acknowledge deps");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![p1.id.as_str().to_string(), p2.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: Some("feature/atomic-test".to_string()),
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed");

    let ep_id_str = result
        .execution_plan_id
        .as_deref()
        .expect("execution_plan_id must be set");
    let ep_id = ExecutionPlanId::from_string(ep_id_str.to_string());

    // 1. ExecutionPlan exists and is linked to the session
    let exec_plan = state
        .execution_plan_repo
        .get_by_id(&ep_id)
        .await
        .expect("repo error")
        .expect("ExecutionPlan must exist after successful apply");
    assert_eq!(exec_plan.session_id, session.id);

    // 2. PlanBranch exists and is linked to the same ExecutionPlan
    let branch = state
        .plan_branch_repo
        .get_by_execution_plan_id(&ep_id)
        .await
        .expect("repo error")
        .expect("PlanBranch must exist for the ExecutionPlan");
    assert_eq!(branch.session_id, session.id);
    assert_eq!(
        branch.execution_plan_id,
        Some(ep_id.clone()),
        "PlanBranch.execution_plan_id must match"
    );

    // 3. All tasks are linked to the same ExecutionPlan (atomic creation)
    assert_eq!(
        result.created_task_ids.len(),
        2,
        "Should create 2 plan tasks"
    );
    for task_id_str in &result.created_task_ids {
        let task_id = ralphx_lib::domain::entities::TaskId::from_string(task_id_str.clone());
        let task = state
            .task_repo
            .get_by_id(&task_id)
            .await
            .expect("repo error")
            .expect("task must exist");
        assert_eq!(
            task.execution_plan_id,
            Some(ep_id.clone()),
            "Each task must be linked to the same ExecutionPlan as the PlanBranch"
        );
    }
}

// ============================================================================
// apply_proposals_core — foreign proposal exclusion
// ============================================================================

/// Mixed local+foreign session: only local proposals get tasks, foreign are
/// skipped, and the remaining count excludes foreign so the session transitions
/// to Accepted.
#[tokio::test]
async fn test_apply_proposals_core_excludes_foreign_proposals() {
    use ralphx_lib::domain::entities::{Priority, Project, ProposalCategory};

    let state = setup_apply_test_state();

    // Create project with a known working directory
    let project = Project::new(
        "Source Project".to_string(),
        "/tmp/local-project".to_string(),
    );
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Create 2 local proposals (no target_project)
    let mut local1 = TaskProposal::new(
        session.id.clone(),
        "Local Task 1".to_string(),
        ProposalCategory::Feature,
        Priority::Medium,
    );
    local1.target_project = None;
    let local1 = state
        .task_proposal_repo
        .create(local1)
        .await
        .expect("Failed to create local proposal 1");

    let mut local2 = TaskProposal::new(
        session.id.clone(),
        "Local Task 2".to_string(),
        ProposalCategory::Feature,
        Priority::Medium,
    );
    local2.target_project = None;
    let local2 = state
        .task_proposal_repo
        .create(local2)
        .await
        .expect("Failed to create local proposal 2");

    // Create 1 foreign proposal targeting a different project directory
    let mut foreign1 = TaskProposal::new(
        session.id.clone(),
        "Foreign Task".to_string(),
        ProposalCategory::Feature,
        Priority::Medium,
    );
    foreign1.target_project = Some("/tmp/other-project".to_string());
    let foreign1 = state
        .task_proposal_repo
        .create(foreign1)
        .await
        .expect("Failed to create foreign proposal");

    // Pass all 3 proposal IDs (including foreign) — apply_proposals_core must filter
    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![
            local1.id.as_str().to_string(),
            local2.id.as_str().to_string(),
            foreign1.id.as_str().to_string(),
        ],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    // Acknowledge dependencies (required for multi-proposal sessions)
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session.id.as_str())
        .await
        .expect("Failed to acknowledge dependencies");

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed");

    // Only the 2 local proposals should produce tasks
    assert_eq!(
        result.created_task_ids.len(),
        2,
        "Expected 2 tasks (one per local proposal); foreign proposal must be skipped"
    );

    // Session must transition to Accepted because remaining local proposals = 0
    let session_id_typed = IdeationSessionId::from_string(session.id.as_str().to_string());
    let updated_session = state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await
        .expect("repo error")
        .expect("Session must exist");

    assert_eq!(
        updated_session.status,
        IdeationSessionStatus::Accepted,
        "Session must be Accepted: all local proposals applied, foreign excluded from remaining count"
    );

    // The foreign proposal must NOT have a created_task_id
    let foreign_updated = state
        .task_proposal_repo
        .get_by_id(&foreign1.id)
        .await
        .expect("repo error")
        .expect("Foreign proposal must still exist");

    assert!(
        foreign_updated.created_task_id.is_none(),
        "Foreign proposal must not have a task created for it"
    );
}

/// All-foreign session: when every proposal is foreign, apply_proposals_core returns
/// tasks_created==0 with a "foreign skipped" message and transitions session to Accepted.
#[tokio::test]
async fn test_apply_proposals_core_all_foreign_returns_early() {
    use ralphx_lib::domain::entities::{Priority, Project, ProposalCategory};

    let state = setup_apply_test_state();

    // Project with a known working directory
    let project = Project::new("Local Project".to_string(), "/tmp/local-only".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    let session = IdeationSession::new(project.id.clone());
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("Failed to create session");

    // Create 2 foreign proposals — both target a different project directory
    let mut foreign1 = TaskProposal::new(
        session.id.clone(),
        "Foreign Task 1".to_string(),
        ProposalCategory::Feature,
        Priority::Medium,
    );
    foreign1.target_project = Some("/tmp/other-project".to_string());
    let foreign1 = state
        .task_proposal_repo
        .create(foreign1)
        .await
        .expect("Failed to create foreign proposal 1");

    let mut foreign2 = TaskProposal::new(
        session.id.clone(),
        "Foreign Task 2".to_string(),
        ProposalCategory::Feature,
        Priority::Medium,
    );
    foreign2.target_project = Some("/tmp/other-project".to_string());
    let foreign2 = state
        .task_proposal_repo
        .create(foreign2)
        .await
        .expect("Failed to create foreign proposal 2");

    let input = ApplyProposalsInput {
        session_id: session.id.as_str().to_string(),
        proposal_ids: vec![
            foreign1.id.as_str().to_string(),
            foreign2.id.as_str().to_string(),
        ],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(&state, input)
        .await
        .expect("apply_proposals_core should succeed for all-foreign sessions");

    // No tasks must be created
    assert_eq!(
        result.tasks_created, 0,
        "All-foreign session must create 0 tasks"
    );
    assert!(
        result.created_task_ids.is_empty(),
        "created_task_ids must be empty for all-foreign session"
    );

    // Message must mention "foreign skipped"
    let msg = result
        .message
        .expect("Expected a message for all-foreign early return");
    assert!(
        msg.contains("foreign skipped"),
        "Message must contain 'foreign skipped', got: {msg}"
    );

    // Session must be transitioned to Accepted
    let session_id_typed = IdeationSessionId::from_string(session.id.as_str().to_string());
    let updated_session = state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await
        .expect("repo error")
        .expect("Session must exist");

    assert_eq!(
        updated_session.status,
        IdeationSessionStatus::Accepted,
        "All-foreign session must transition to Accepted"
    );
}
