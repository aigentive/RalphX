// Integration tests for agentic client functionality
// Tests the full flow of spawning agents and processing responses

use std::sync::Arc;

use chrono::Utc;
use ralphx_lib::application::AppState;
use ralphx_lib::domain::agents::{AgentConfig, AgentRole, AgenticClient, ClientType};
use ralphx_lib::domain::entities::{ChatConversation, Persona, PersonaId, PersonaStatus, Project};
use ralphx_lib::domain::state_machine::AgentSpawner;
use ralphx_lib::application::agents::AgenticClientSpawner;
use ralphx_lib::infrastructure::{ClaudeCodeClient, MockAgenticClient, MockCallType};
use ralphx_lib::testing::test_prompts;

async fn seed_bound_active_persona() -> (AppState, tempfile::TempDir, String) {
    let state = AppState::new_test();
    // Never point test projects at the real checkout: git-touching flows
    // (e.g. the freshness auto-commit) would mutate the developer's repo.
    let project_dir = tempfile::tempdir().expect("temp project dir");
    let project = Project::new(
        "Excluded agent persona project".to_string(),
        project_dir.path().to_string_lossy().to_string(),
    );
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");

    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from("excluded-agent-persona"),
        artifact_id: None,

        project_id: None,
        slug: "excluded-agent-persona".to_string(),
        name: "Excluded Agent Persona".to_string(),
        description: "must not reach excluded spawns".to_string(),
        content: "<ralphx_agent_persona>\nExcluded persona body\n</ralphx_agent_persona>"
            .to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "excluded-agent-persona-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed active persona");

    let mut conversation = ChatConversation::new_project(project.id);
    conversation.persona_id = Some(persona.id.to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("bind active persona to conversation");

    (state, project_dir, persona.content)
}

// ============================================================================
// MockAgenticClient Integration Tests
// ============================================================================

#[tokio::test]
async fn test_mock_client_spawn_and_complete_flow() {
    let client = MockAgenticClient::new();

    // Spawn a worker agent
    let config = AgentConfig::worker("Implement the feature");
    let handle = client.spawn_agent(config).await.unwrap();

    assert_eq!(handle.role, AgentRole::Worker);
    assert_eq!(handle.client_type, ClientType::Mock);

    // Wait for completion
    let output = client.wait_for_completion(&handle).await.unwrap();

    assert!(output.success);
    assert_eq!(output.content, "MOCK_COMPLETION");
}

#[tokio::test]
async fn test_mock_client_configured_responses() {
    let client = MockAgenticClient::new();
    let handle = ralphx_lib::domain::agents::AgentHandle::mock(AgentRole::Worker);

    // Configure responses
    client
        .when_prompt_contains("implement", "Feature implemented successfully")
        .await;
    client
        .when_prompt_contains("review", "Code looks good, approved")
        .await;

    // Test matching prompts
    let response1 = client
        .send_prompt(&handle, "implement login")
        .await
        .unwrap();
    assert_eq!(response1.content, "Feature implemented successfully");

    let response2 = client.send_prompt(&handle, "review my code").await.unwrap();
    assert_eq!(response2.content, "Code looks good, approved");
}

#[tokio::test]
async fn test_mock_client_records_all_calls() {
    let client = MockAgenticClient::new();

    // Spawn multiple agents
    let config1 = AgentConfig::worker("Task 1");
    let config2 = AgentConfig::reviewer("Review task");
    let config3 = AgentConfig::qa_prep("Prepare QA");

    let _ = client.spawn_agent(config1).await.unwrap();
    let _ = client.spawn_agent(config2).await.unwrap();
    let _ = client.spawn_agent(config3).await.unwrap();

    // Verify call recording
    let calls = client.get_spawn_calls().await;
    assert_eq!(calls.len(), 3);

    // Verify roles
    let roles: Vec<_> = calls
        .iter()
        .map(|c| {
            if let MockCallType::Spawn { role, .. } = &c.call_type {
                role.clone()
            } else {
                panic!("Expected Spawn call")
            }
        })
        .collect();

    assert!(roles.contains(&AgentRole::Worker));
    assert!(roles.contains(&AgentRole::Reviewer));
    assert!(roles.contains(&AgentRole::QaPrep));
}

#[tokio::test]
async fn test_mock_client_with_test_prompts() {
    let client = MockAgenticClient::new();
    let handle = ralphx_lib::domain::agents::AgentHandle::mock(AgentRole::Worker);

    // Use test prompts constants
    client
        .when_prompt_contains("TEST_ECHO_OK", test_prompts::expected::ECHO_OK)
        .await;

    let response = client
        .send_prompt(&handle, test_prompts::ECHO_MARKER)
        .await
        .unwrap();
    test_prompts::assert_marker(&response.content, test_prompts::expected::ECHO_OK);
}

#[tokio::test]
async fn test_mock_client_stop_agent() {
    let client = MockAgenticClient::new();

    let config = AgentConfig::worker("Long running task");
    let handle = client.spawn_agent(config).await.unwrap();

    // Stop should succeed
    let result = client.stop_agent(&handle).await;
    assert!(result.is_ok());

    // Verify stop was recorded
    let calls = client.get_calls().await;
    let stop_calls: Vec<_> = calls
        .iter()
        .filter(|c| matches!(c.call_type, MockCallType::Stop { .. }))
        .collect();
    assert_eq!(stop_calls.len(), 1);
}

// ============================================================================
// ClaudeCodeClient Integration Tests
// ============================================================================

#[tokio::test]
async fn test_claude_client_availability_check() {
    let client = ClaudeCodeClient::new();

    // Just verify is_available doesn't panic
    let result = client.is_available().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_claude_client_capabilities() {
    let client = ClaudeCodeClient::new();
    let caps = client.capabilities();

    assert_eq!(caps.client_type, ClientType::ClaudeCode);
    assert!(caps.supports_shell);
    assert!(caps.supports_filesystem);
    assert!(caps.supports_streaming);
    assert!(caps.supports_mcp);
    assert!(caps.max_context_tokens > 0);
    assert!(!caps.models.is_empty());
}

#[tokio::test]
async fn test_claude_client_with_nonexistent_cli() {
    let client = ClaudeCodeClient::new().with_cli_path("/nonexistent/path/to/claude_12345");

    // Should report not available
    let available = client.is_available().await.unwrap();
    assert!(!available);

    // Spawn should fail gracefully
    let config = AgentConfig::worker("test");
    let result = client.spawn_agent(config).await;
    assert!(result.is_err());
}

// ============================================================================
// Claude Spawn Policy (tests must not spawn real agents)
// ============================================================================

#[tokio::test]
async fn test_claude_spawn_blocked_in_tests() {
    // NOTE: Integration tests don't have cfg!(test) = true, so we must
    // explicitly enable test mode via environment variable
    let _test_mode = crate::support::env::EnvVarGuard::set("RALPHX_TEST_MODE", "1");

    let client = ClaudeCodeClient::new();
    let config = AgentConfig::worker("test");

    let result = client.spawn_agent(config).await;
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(ralphx_lib::domain::agents::AgentError::SpawnNotAllowed(_))
    ));
}

// ============================================================================
// AgenticClientSpawner Integration Tests
// ============================================================================

#[tokio::test]
async fn test_spawner_implements_agent_spawner_trait() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    // Use the trait methods
    spawner.spawn("worker", "task-123").await;
    spawner.spawn_background("qa-prep", "task-456").await;
    spawner.wait_for("worker", "task-123").await;
    spawner.stop("worker", "task-123").await;

    // Verify spawns were recorded
    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 2); // spawn + spawn_background
}

#[tokio::test]
async fn test_spawner_maps_roles_correctly() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    // Spawn different types
    spawner.spawn("worker", "t1").await;
    spawner.spawn("reviewer", "t2").await;
    spawner.spawn("qa-prep", "t3").await;
    spawner.spawn("qa-refiner", "t4").await;
    spawner.spawn("qa-tester", "t5").await;
    spawner.spawn("custom-agent", "t6").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 6);

    let roles: Vec<_> = calls
        .iter()
        .map(|c| {
            if let MockCallType::Spawn { role, .. } = &c.call_type {
                role.clone()
            } else {
                panic!("Expected Spawn call")
            }
        })
        .collect();

    assert_eq!(roles[0], AgentRole::Worker);
    assert_eq!(roles[1], AgentRole::Reviewer);
    assert_eq!(roles[2], AgentRole::QaPrep);
    assert_eq!(roles[3], AgentRole::QaRefiner);
    assert_eq!(roles[4], AgentRole::QaTester);
    assert_eq!(roles[5], AgentRole::Custom("custom-agent".to_string()));
}

#[tokio::test]
async fn background_utility_spawns_have_no_persona() {
    let (_state, _project_dir, bound_persona_block) = seed_bound_active_persona().await;
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    for agent_type in [
        "ralphx-utility-session-namer",
        "ralphx-memory-maintainer",
        "ralphx-memory-capture",
        "qa-prep",
        "ralphx-project-analyzer",
    ] {
        spawner.spawn(agent_type, "task-with-bound-persona").await;
    }

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 5, "every background utility should spawn");
    for call in calls {
        let MockCallType::Spawn { prompt, .. } = call.call_type else {
            panic!("expected background utility spawn");
        };
        assert!(
            !prompt.contains("<ralphx_agent_persona>")
                && !prompt.contains(bound_persona_block.as_str()),
            "background utility final AgentConfig prompt must not inherit a conversation persona"
        );
    }
}

#[tokio::test]
async fn pipeline_agent_spawn_has_no_persona() {
    let (_state, _project_dir, bound_persona_block) = seed_bound_active_persona().await;
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    for agent_type in ["worker", "reviewer", "merger"] {
        spawner.spawn(agent_type, "task-with-bound-persona").await;
    }

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 3, "worker, reviewer, and merger should spawn");
    for call in calls {
        let MockCallType::Spawn { prompt, .. } = call.call_type else {
            panic!("expected pipeline agent spawn");
        };
        assert!(
            !prompt.contains("<ralphx_agent_persona>")
                && !prompt.contains(bound_persona_block.as_str()),
            "pipeline final AgentConfig prompt must not inherit a conversation persona"
        );
    }
}
