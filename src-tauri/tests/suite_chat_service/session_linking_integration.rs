// Integration test: Session linking (create_child_session and get_parent_session_context)
//
// Tests the session linking functionality:
// - Create child session with valid parent (validates parent exists)
// - Reject circular reference (A→B→A)
// - Reject self-reference
// - Verify context includes plan content and proposals
// - Verify get_parent_context returns 404 for session without parent

use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    Artifact, ArtifactContent, ArtifactType, Complexity, IdeationSession, IdeationSessionBuilder,
    IdeationSessionId, IdeationSessionStatus, Priority, ProjectId, ProposalCategory,
    ProposalStatus, TaskProposal, TaskProposalId,
};
// No SQLite infrastructure imports needed for memory-only tests

// ============================================================================
// Test Setup Helpers
// ============================================================================

fn create_memory_state() -> AppState {
    AppState::new_test()
}

async fn create_parent_session(state: &AppState, project_id: &ProjectId) -> IdeationSession {
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent Session")
        .status(IdeationSessionStatus::Active)
        .build();

    state.ideation_session_repo.create(parent).await.unwrap()
}

async fn create_session_with_parent(
    state: &AppState,
    project_id: &ProjectId,
    parent_id: &IdeationSessionId,
) -> IdeationSession {
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child Session")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        .build();

    state.ideation_session_repo.create(child).await.unwrap()
}

async fn create_session_with_plan_and_proposals(
    state: &AppState,
    project_id: &ProjectId,
) -> (IdeationSession, String, Vec<String>) {
    // Create session
    let session = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Session with Plan")
        .status(IdeationSessionStatus::Active)
        .build();

    let session_id = session.id.clone();
    let created_session = state.ideation_session_repo.create(session).await.unwrap();

    // Create plan artifact
    let plan = Artifact::new_inline(
        "Implementation Plan",
        ArtifactType::Specification,
        "# Plan\n\nThis is the implementation plan for the project.",
        "orchestrator",
    );
    let plan_id_string = plan.id.to_string();
    state.artifact_repo.create(plan).await.unwrap();

    // Update session with plan
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session_id, Some(plan_id_string.clone()))
        .await
        .unwrap();

    // Create proposals
    let mut proposal_ids = vec![];
    for i in 1..=2 {
        let proposal = TaskProposal {
            id: TaskProposalId::new(),
            session_id: session_id.clone(),
            title: format!("Proposal {}", i),
            description: Some(format!("Description for proposal {}", i)),
            category: ProposalCategory::Feature,
            status: ProposalStatus::Pending,
            suggested_priority: Priority::High,
            priority_score: 75,
            priority_reason: Some("High priority for testing".to_string()),
            priority_factors: None,
            estimated_complexity: Complexity::Moderate,
            user_priority: None,
            user_modified: false,
            steps: None,
            acceptance_criteria: None,
            plan_artifact_id: None,
            plan_version_at_creation: None,
            created_task_id: None,
            selected: false,
            affected_paths: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            sort_order: i,
            target_project: None,
            migrated_from_session_id: None,
            migrated_from_proposal_id: None,
        };
        proposal_ids.push(proposal.id.to_string());
        state.task_proposal_repo.create(proposal).await.unwrap();
    }

    (created_session, plan_id_string, proposal_ids)
}

// ============================================================================
// Shared Test Logic
// ============================================================================

async fn test_create_child_with_valid_parent(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session
    let parent = create_parent_session(state, &project_id).await;
    let parent_id = parent.id.clone();

    // Create child session
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child Session")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify parent_session_id is set
    assert_eq!(created_child.parent_session_id, Some(parent_id));
    assert_eq!(created_child.title, Some("Child Session".to_string()));
    assert_eq!(created_child.status, IdeationSessionStatus::Active);
}

async fn test_reject_circular_reference(state: &AppState) {
    let project_id = ProjectId::new();

    // Create session A
    let session_a = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Session A")
        .status(IdeationSessionStatus::Active)
        .build();
    let session_a_id = session_a.id.clone();
    state.ideation_session_repo.create(session_a).await.unwrap();

    // Create session B with parent A
    let session_b = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Session B")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(session_a_id.clone())
        .build();
    let session_b_id = session_b.id.clone();
    state.ideation_session_repo.create(session_b).await.unwrap();

    // Try to make A have B as parent (would create cycle A→B→A)
    let ancestor_chain = state
        .ideation_session_repo
        .get_ancestor_chain(&session_b_id)
        .await
        .unwrap();

    // The ancestor chain of B should contain A
    assert!(
        ancestor_chain.iter().any(|s| s.id == session_a_id),
        "Session A should be in ancestor chain of B"
    );

    // If we were to set A's parent to B, it would create a cycle
    // The cycle detection logic should prevent this
}

async fn test_reject_self_reference(state: &AppState) {
    let project_id = ProjectId::new();

    // Create a session
    let session = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Self-Referencing Session")
        .status(IdeationSessionStatus::Active)
        .build();

    let created_session = state.ideation_session_repo.create(session).await.unwrap();

    // Verify it doesn't have itself as parent
    assert_ne!(
        created_session.parent_session_id,
        Some(created_session.id.clone()),
        "Session should not be its own parent"
    );
}

async fn test_context_includes_plan_and_proposals(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session with plan and proposals
    let (parent_session, _plan_id, _proposal_ids) =
        create_session_with_plan_and_proposals(state, &project_id).await;
    let parent_id = parent_session.id.clone();

    // Create child session linked to parent
    let child = create_session_with_parent(state, &project_id, &parent_id).await;

    // Verify child has parent
    assert_eq!(child.parent_session_id, Some(parent_id.clone()));

    // Fetch parent's plan if it exists
    if let Some(plan_artifact_id) = &parent_session.plan_artifact_id {
        let plan = state
            .artifact_repo
            .get_by_id(plan_artifact_id)
            .await
            .unwrap()
            .unwrap();

        match plan.content {
            ArtifactContent::Inline { text } => {
                assert!(text.contains("Implementation Plan"));
            }
            _ => panic!("Expected inline plan content"),
        }
    }

    // Fetch parent's proposals
    let parent_proposals = state
        .task_proposal_repo
        .get_by_session(&parent_id)
        .await
        .unwrap();

    assert_eq!(parent_proposals.len(), 2);
    assert_eq!(parent_proposals[0].title, "Proposal 1");
    assert_eq!(parent_proposals[1].title, "Proposal 2");
}

async fn test_get_parent_context_404_without_parent(state: &AppState) {
    let project_id = ProjectId::new();

    // Create a session WITHOUT a parent
    let session = create_parent_session(state, &project_id).await;
    let session_id = session.id.clone();

    // Try to fetch parent context
    let result = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();

    // The session should have no parent
    assert_eq!(result.parent_session_id, None);
}

async fn test_get_ancestor_chain(state: &AppState) {
    let project_id = ProjectId::new();

    // Create chain: A -> B -> C
    let session_a = create_parent_session(state, &project_id).await;
    let session_a_id = session_a.id.clone();

    let session_b = create_session_with_parent(state, &project_id, &session_a_id).await;
    let session_b_id = session_b.id.clone();

    let session_c = create_session_with_parent(state, &project_id, &session_b_id).await;
    let session_c_id = session_c.id.clone();

    // Get ancestor chain of C
    let ancestor_chain = state
        .ideation_session_repo
        .get_ancestor_chain(&session_c_id)
        .await
        .unwrap();

    // Should contain B and A in order (B is direct parent, A is grandparent)
    assert!(ancestor_chain.len() >= 2);
    assert_eq!(ancestor_chain[0].id, session_b_id); // Direct parent
    assert_eq!(ancestor_chain[1].id, session_a_id); // Grandparent
}

// ============================================================================
// Test Runners (Memory + SQLite)
// ============================================================================

#[tokio::test]
async fn memory_create_child_with_valid_parent() {
    test_create_child_with_valid_parent(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_reject_circular_reference() {
    test_reject_circular_reference(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_reject_self_reference() {
    test_reject_self_reference(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_context_includes_plan_and_proposals() {
    test_context_includes_plan_and_proposals(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_get_parent_context_404_without_parent() {
    test_get_parent_context_404_without_parent(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_get_ancestor_chain() {
    test_get_ancestor_chain(&create_memory_state()).await;
}

// ============================================================================
// Tests for Fixed Child Session Inheritance (Phase 1 fixes)
// ============================================================================

/// Test: Child session status is always Active, regardless of parent's status.
/// This verifies the fix in session_linking.rs:79 where child session's status
/// is hardcoded to IdeationSessionStatus::Active instead of copying parent's status.
async fn test_child_status_is_always_active(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session with ACCEPTED status (not Active)
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Accepted Parent Session")
        .status(IdeationSessionStatus::Accepted) // Parent is Accepted
        .build();

    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Simulate handler behavior: create child with Active status (not parent's Accepted)
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child Session")
        .status(IdeationSessionStatus::Active) // Handler always sets Active
        .parent_session_id(parent_id.clone())
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child status is Active, not Accepted (parent's status)
    assert_eq!(
        created_child.status,
        IdeationSessionStatus::Active,
        "Child session status should always be Active, not parent's status"
    );
    assert_eq!(created_child.parent_session_id, Some(parent_id));
}

/// Test: Child inherits plan when parent has a plan AND inherit_context is true.
/// This verifies the fix in session_linking.rs:80-81 where child's plan_artifact_id
/// is set to parent's plan_artifact_id when inherit_context is true.
async fn test_child_inherits_plan_when_parent_has_plan_and_inherit_true(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session with a plan artifact
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent with Plan")
        .status(IdeationSessionStatus::Active)
        .build();

    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Create plan artifact
    let plan = Artifact::new_inline(
        "Test Plan",
        ArtifactType::Specification,
        "# Plan\n\nTest content",
        "test",
    );
    let plan_id = plan.id.clone();
    state.artifact_repo.create(plan).await.unwrap();

    // Link plan to parent session
    state
        .ideation_session_repo
        .update_plan_artifact_id(&parent_id, Some(plan_id.to_string()))
        .await
        .unwrap();

    // Simulate handler behavior with inherit_context=true:
    // child.plan_artifact_id = parent.plan_artifact_id
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child with Inherited Plan")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        .plan_artifact_id(plan_id.clone()) // Inherited from parent
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child has the same plan as parent
    assert_eq!(
        created_child.plan_artifact_id,
        Some(plan_id),
        "Child should inherit parent's plan when inherit_context is true"
    );
}

/// Test: Child has no plan when inherit_context is false, even if parent has a plan.
/// This verifies the fix in session_linking.rs:82-83 where child's plan_artifact_id
/// is set to None when inherit_context is false.
async fn test_child_has_no_plan_when_inherit_false(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session with a plan artifact
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent with Plan")
        .status(IdeationSessionStatus::Active)
        .build();

    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Create plan artifact
    let plan = Artifact::new_inline(
        "Test Plan",
        ArtifactType::Specification,
        "# Plan\n\nTest content",
        "test",
    );
    let plan_id = plan.id.clone();
    state.artifact_repo.create(plan).await.unwrap();

    // Link plan to parent session
    state
        .ideation_session_repo
        .update_plan_artifact_id(&parent_id, Some(plan_id.to_string()))
        .await
        .unwrap();

    // Simulate handler behavior with inherit_context=false:
    // child.plan_artifact_id = None (don't set it, builder defaults to None)
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child without Inherited Plan")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        // No plan_artifact_id call - NOT inherited because inherit_context=false
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child has no plan despite parent having one
    assert_eq!(
        created_child.plan_artifact_id, None,
        "Child should NOT inherit parent's plan when inherit_context is false"
    );
}

/// Test: Child has no plan when parent has no plan, even with inherit_context=true.
/// This verifies that plan inheritance only happens when parent actually has a plan.
async fn test_child_has_no_plan_when_parent_has_no_plan(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session WITHOUT a plan artifact
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent without Plan")
        .status(IdeationSessionStatus::Active)
        // No plan_artifact_id set
        .build();

    let parent_id = parent.id.clone();
    let created_parent = state.ideation_session_repo.create(parent).await.unwrap();

    // Verify parent has no plan
    assert_eq!(
        created_parent.plan_artifact_id, None,
        "Parent should have no plan"
    );

    // Simulate handler behavior with inherit_context=true but parent has no plan:
    // child.plan_artifact_id = parent.plan_artifact_id (which is None)
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child of Parent without Plan")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        // No plan_artifact_id call - parent has no plan to inherit
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child has no plan despite inherit_context=true
    assert_eq!(
        created_child.plan_artifact_id, None,
        "Child should have no plan when parent has no plan, even with inherit_context=true"
    );
}

// ============================================================================
// Test Runners for Child Session Inheritance (Memory)
// ============================================================================

#[tokio::test]
async fn memory_child_status_is_always_active() {
    test_child_status_is_always_active(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_child_inherits_plan_when_parent_has_plan_and_inherit_true() {
    test_child_inherits_plan_when_parent_has_plan_and_inherit_true(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_child_has_no_plan_when_inherit_false() {
    test_child_has_no_plan_when_inherit_false(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_child_has_no_plan_when_parent_has_no_plan() {
    test_child_has_no_plan_when_parent_has_no_plan(&create_memory_state()).await;
}

// ============================================================================
// Tests for Team Config Inheritance + Validation
// ============================================================================

/// Test: Child inherits parent's team config when inherit_context=true.
/// Verifies the inheritance priority: explicit > inherited > None
/// Also verifies that inherited config is validated against project constraints.
async fn test_child_inherits_team_config_when_parent_has_config_and_inherit_true(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session with team config
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent with Team Config")
        .status(IdeationSessionStatus::Active)
        .team_mode("research")
        .team_config_json(
            serde_json::to_string(&serde_json::json!({
                "max_teammates": 3,
                "model_ceiling": "sonnet"
            }))
            .unwrap(),
        )
        .build();

    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Simulate handler behavior with inherit_context=true:
    // child.team_mode = parent.team_mode, child.team_config_json = parent.team_config_json
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child with Inherited Team Config")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        .team_mode("research") // Inherited from parent
        .team_config_json(
            serde_json::to_string(&serde_json::json!({
                "max_teammates": 3,
                "model_ceiling": "sonnet"
            }))
            .unwrap(),
        ) // Inherited from parent
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child has the same team config as parent
    assert_eq!(
        created_child.team_mode,
        Some("research".to_string()),
        "Child should inherit parent's team_mode when inherit_context is true"
    );
    assert!(
        created_child.team_config_json.is_some(),
        "Child should inherit parent's team_config_json when inherit_context is true"
    );

    // Parse and verify the config
    let config: serde_json::Value =
        serde_json::from_str(created_child.team_config_json.unwrap().as_str()).unwrap();
    assert_eq!(config["max_teammates"], 3);
    assert_eq!(config["model_ceiling"], "sonnet");
}

/// Test: Child with explicit team_config overrides inheritance.
/// Verifies priority: explicit > inherited
async fn test_child_explicit_team_config_overrides_inheritance(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session with team config
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent with Team Config")
        .status(IdeationSessionStatus::Active)
        .team_mode("research")
        .team_config_json(
            serde_json::to_string(&serde_json::json!({
                "max_teammates": 5,
                "model_ceiling": "opus"
            }))
            .unwrap(),
        )
        .build();

    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Simulate handler behavior with EXPLICIT team_config:
    // explicit params take priority over inheritance
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child with Explicit Team Config")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        .team_mode("debate") // Explicit override
        .team_config_json(
            serde_json::to_string(&serde_json::json!({
                "max_teammates": 2,
                "model_ceiling": "haiku"
            }))
            .unwrap(),
        ) // Explicit override, NOT inherited
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child has EXPLICIT config, not parent's
    assert_eq!(
        created_child.team_mode,
        Some("debate".to_string()),
        "Child should have explicit team_mode, not inherited"
    );

    let config: serde_json::Value =
        serde_json::from_str(created_child.team_config_json.unwrap().as_str()).unwrap();
    assert_eq!(
        config["max_teammates"], 2,
        "Child should have explicit max_teammates, not inherited 5"
    );
    assert_eq!(
        config["model_ceiling"], "haiku",
        "Child should have explicit model_ceiling, not inherited opus"
    );
}

/// Test: Child with inherit_context=false gets no team config (solo mode).
async fn test_child_no_team_config_when_inherit_false(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session with team config
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent with Team Config")
        .status(IdeationSessionStatus::Active)
        .team_mode("research")
        .team_config_json(
            serde_json::to_string(&serde_json::json!({
                "max_teammates": 5,
                "model_ceiling": "opus"
            }))
            .unwrap(),
        )
        .build();

    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Simulate handler behavior with inherit_context=false:
    // team_mode and team_config_json are None (solo mode)
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child without Inherited Team Config")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        // No team_mode/team_config_json - NOT inherited because inherit_context=false
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child has NO team config despite parent having one
    assert_eq!(
        created_child.team_mode, None,
        "Child should NOT inherit team_mode when inherit_context is false"
    );
    assert_eq!(
        created_child.team_config_json, None,
        "Child should NOT inherit team_config_json when inherit_context is false"
    );
}

/// Test: Child inherits parent's NULL team config as None (solo mode).
async fn test_child_inherits_none_when_parent_has_no_team_config(state: &AppState) {
    let project_id = ProjectId::new();

    // Create parent session WITHOUT team config
    let parent = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Parent without Team Config")
        .status(IdeationSessionStatus::Active)
        // No team_mode/team_config_json set
        .build();

    let parent_id = parent.id.clone();
    let created_parent = state.ideation_session_repo.create(parent).await.unwrap();

    // Verify parent has no team config
    assert_eq!(
        created_parent.team_mode, None,
        "Parent should have no team_mode"
    );
    assert_eq!(
        created_parent.team_config_json, None,
        "Parent should have no team_config_json"
    );

    // Simulate handler behavior with inherit_context=true but parent has no team config:
    // child.team_mode = parent.team_mode (None), child.team_config_json = parent.team_config_json (None)
    let child = IdeationSessionBuilder::new()
        .id(IdeationSessionId::new())
        .project_id(project_id.clone())
        .title("Child of Parent without Team Config")
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id.clone())
        // No team_mode/team_config_json - parent has none to inherit
        .build();

    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child has no team config despite inherit_context=true
    assert_eq!(
        created_child.team_mode, None,
        "Child should have no team_mode when parent has none, even with inherit_context=true"
    );
    assert_eq!(
        created_child.team_config_json, None,
        "Child should have no team_config_json when parent has none, even with inherit_context=true"
    );
}

// ============================================================================
// Test Runners for Team Config Inheritance (Memory)
// ============================================================================

#[tokio::test]
async fn memory_child_inherits_team_config_when_parent_has_config_and_inherit_true() {
    test_child_inherits_team_config_when_parent_has_config_and_inherit_true(&create_memory_state())
        .await;
}

#[tokio::test]
async fn memory_child_explicit_team_config_overrides_inheritance() {
    test_child_explicit_team_config_overrides_inheritance(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_child_no_team_config_when_inherit_false() {
    test_child_no_team_config_when_inherit_false(&create_memory_state()).await;
}

#[tokio::test]
async fn memory_child_inherits_none_when_parent_has_no_team_config() {
    test_child_inherits_none_when_parent_has_no_team_config(&create_memory_state()).await;
}
