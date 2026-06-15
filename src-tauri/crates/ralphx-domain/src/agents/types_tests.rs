use super::*;

// AgentRole tests
#[test]
fn test_agent_role_worker() {
    let role = AgentRole::Worker;
    assert_eq!(role.to_string(), "worker");
}

#[test]
fn test_agent_role_reviewer() {
    let role = AgentRole::Reviewer;
    assert_eq!(role.to_string(), "reviewer");
}

#[test]
fn test_agent_role_qa_prep() {
    let role = AgentRole::QaPrep;
    assert_eq!(role.to_string(), "qa-prep");
}

#[test]
fn test_agent_role_qa_refiner() {
    let role = AgentRole::QaRefiner;
    assert_eq!(role.to_string(), "qa-refiner");
}

#[test]
fn test_agent_role_qa_tester() {
    let role = AgentRole::QaTester;
    assert_eq!(role.to_string(), "qa-tester");
}

#[test]
fn test_agent_role_supervisor() {
    let role = AgentRole::Supervisor;
    assert_eq!(role.to_string(), "supervisor");
}

#[test]
fn test_agent_role_custom() {
    let role = AgentRole::Custom("my-custom-agent".to_string());
    assert_eq!(role.to_string(), "my-custom-agent");
}

#[test]
fn test_agent_role_equality() {
    assert_eq!(AgentRole::Worker, AgentRole::Worker);
    assert_ne!(AgentRole::Worker, AgentRole::Reviewer);
}

#[test]
fn test_agent_role_clone() {
    let role = AgentRole::Custom("test".to_string());
    let cloned = role.clone();
    assert_eq!(role, cloned);
}

#[test]
fn test_agent_role_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(AgentRole::Worker);
    set.insert(AgentRole::Reviewer);
    assert!(set.contains(&AgentRole::Worker));
    assert!(!set.contains(&AgentRole::Supervisor));
}

// ClientType tests
#[test]
fn test_client_type_claude_code() {
    let client = ClientType::ClaudeCode;
    assert_eq!(client.to_string(), "claude-code");
}

#[test]
fn test_client_type_codex() {
    let client = ClientType::Codex;
    assert_eq!(client.to_string(), "codex");
}

#[test]
fn test_client_type_gemini() {
    let client = ClientType::Gemini;
    assert_eq!(client.to_string(), "gemini");
}

#[test]
fn test_client_type_mock() {
    let client = ClientType::Mock;
    assert_eq!(client.to_string(), "mock");
}

#[test]
fn test_client_type_custom() {
    let client = ClientType::Custom("my-custom-client".to_string());
    assert_eq!(client.to_string(), "my-custom-client");
}

#[test]
fn test_client_type_equality() {
    assert_eq!(ClientType::Mock, ClientType::Mock);
    assert_ne!(ClientType::Mock, ClientType::ClaudeCode);
}

// AgentConfig tests
#[test]
fn test_agent_config_default() {
    let config = AgentConfig::default();
    assert_eq!(config.role, AgentRole::Worker);
    assert!(config.prompt.is_empty());
    assert_eq!(config.plugin_dir, Some(PathBuf::from("./plugins/app")));
    assert!(config.agent.is_none());
    assert!(config.model.is_none());
    assert!(config.cli_path_override.is_none());
    assert!(config.max_tokens.is_none());
    assert!(config.timeout_secs.is_none());
    assert!(config.env.is_empty());
}

#[test]
fn test_agent_config_worker() {
    let config = AgentConfig::worker("Do some work");
    assert_eq!(config.role, AgentRole::Worker);
    assert_eq!(config.prompt, "Do some work");
}

#[test]
fn test_agent_config_reviewer() {
    let config = AgentConfig::reviewer("Review this code");
    assert_eq!(config.role, AgentRole::Reviewer);
    assert_eq!(config.prompt, "Review this code");
}

#[test]
fn test_agent_config_qa_prep() {
    let config = AgentConfig::qa_prep("Prepare QA criteria");
    assert_eq!(config.role, AgentRole::QaPrep);
    assert_eq!(config.prompt, "Prepare QA criteria");
}

#[test]
fn test_agent_config_with_working_dir() {
    let config = AgentConfig::default().with_working_dir("/tmp/work");
    assert_eq!(config.working_directory, PathBuf::from("/tmp/work"));
}

#[test]
fn test_agent_config_with_model() {
    let config = AgentConfig::default().with_model("claude-sonnet-4-5");
    assert_eq!(config.model, Some("claude-sonnet-4-5".to_string()));
}

#[test]
fn test_agent_config_with_cli_path_override() {
    let config = AgentConfig::default().with_cli_path_override("/managed/bin/codex");
    assert_eq!(
        config.cli_path_override,
        Some(PathBuf::from("/managed/bin/codex"))
    );
}

#[test]
fn test_agent_config_with_timeout() {
    let config = AgentConfig::default().with_timeout(300);
    assert_eq!(config.timeout_secs, Some(300));
}

#[test]
fn test_agent_config_with_env() {
    let config = AgentConfig::default()
        .with_env("API_KEY", "secret")
        .with_env("DEBUG", "true");
    assert_eq!(config.env.get("API_KEY"), Some(&"secret".to_string()));
    assert_eq!(config.env.get("DEBUG"), Some(&"true".to_string()));
}

#[test]
fn test_agent_config_builder_chain() {
    let config = AgentConfig::worker("test")
        .with_working_dir("/tmp")
        .with_model("sonnet")
        .with_timeout(60)
        .with_env("KEY", "value");

    assert_eq!(config.role, AgentRole::Worker);
    assert_eq!(config.prompt, "test");
    assert_eq!(config.working_directory, PathBuf::from("/tmp"));
    assert_eq!(config.model, Some("sonnet".to_string()));
    assert_eq!(config.timeout_secs, Some(60));
    assert_eq!(config.env.get("KEY"), Some(&"value".to_string()));
}

// AgentHandle tests
#[test]
fn test_agent_handle_new() {
    let handle = AgentHandle::new(ClientType::ClaudeCode, AgentRole::Worker);
    assert_eq!(handle.client_type, ClientType::ClaudeCode);
    assert_eq!(handle.role, AgentRole::Worker);
    assert!(!handle.id.is_empty());
}

#[test]
fn test_agent_handle_mock() {
    let handle = AgentHandle::mock(AgentRole::Reviewer);
    assert_eq!(handle.client_type, ClientType::Mock);
    assert_eq!(handle.role, AgentRole::Reviewer);
}

#[test]
fn test_agent_handle_with_id() {
    let handle = AgentHandle::with_id("custom-id", ClientType::Mock, AgentRole::Worker);
    assert_eq!(handle.id, "custom-id");
}

#[test]
fn test_agent_handle_unique_ids() {
    let h1 = AgentHandle::new(ClientType::Mock, AgentRole::Worker);
    let h2 = AgentHandle::new(ClientType::Mock, AgentRole::Worker);
    assert_ne!(h1.id, h2.id);
}

#[test]
fn test_agent_handle_equality() {
    let h1 = AgentHandle::with_id("same-id", ClientType::Mock, AgentRole::Worker);
    let h2 = AgentHandle::with_id("same-id", ClientType::Mock, AgentRole::Worker);
    assert_eq!(h1, h2);
}

#[test]
fn test_agent_handle_hash() {
    use std::collections::HashSet;
    let h1 = AgentHandle::with_id("id-1", ClientType::Mock, AgentRole::Worker);
    let h2 = AgentHandle::with_id("id-2", ClientType::Mock, AgentRole::Worker);
    let mut set = HashSet::new();
    set.insert(h1.clone());
    assert!(set.contains(&h1));
    assert!(!set.contains(&h2));
}

// AgentOutput tests
#[test]
fn test_agent_output_default() {
    let output = AgentOutput::default();
    assert!(!output.success);
    assert!(output.content.is_empty());
    assert!(output.exit_code.is_none());
    assert!(output.duration_ms.is_none());
}

#[test]
fn test_agent_output_success() {
    let output = AgentOutput::success("Task completed");
    assert!(output.success);
    assert_eq!(output.content, "Task completed");
    assert_eq!(output.exit_code, Some(0));
}

#[test]
fn test_agent_output_failed() {
    let output = AgentOutput::failed("Error occurred", 1);
    assert!(!output.success);
    assert_eq!(output.content, "Error occurred");
    assert_eq!(output.exit_code, Some(1));
}

#[test]
fn test_agent_output_with_duration() {
    let output = AgentOutput::success("test").with_duration(5000);
    assert_eq!(output.duration_ms, Some(5000));
}

// AgentResponse tests
#[test]
fn test_agent_response_default() {
    let response = AgentResponse::default();
    assert!(response.content.is_empty());
    assert!(response.model.is_none());
    assert!(response.tokens_used.is_none());
}

#[test]
fn test_agent_response_new() {
    let response = AgentResponse::new("Hello, world!");
    assert_eq!(response.content, "Hello, world!");
}

#[test]
fn test_agent_response_with_model() {
    let response = AgentResponse::new("test").with_model("claude-sonnet");
    assert_eq!(response.model, Some("claude-sonnet".to_string()));
}

#[test]
fn test_agent_response_with_tokens() {
    let response = AgentResponse::new("test").with_tokens(150);
    assert_eq!(response.tokens_used, Some(150));
}

#[test]
fn test_agent_response_builder_chain() {
    let response = AgentResponse::new("test")
        .with_model("opus")
        .with_tokens(200);
    assert_eq!(response.content, "test");
    assert_eq!(response.model, Some("opus".to_string()));
    assert_eq!(response.tokens_used, Some(200));
}

#[test]
fn test_agent_config_with_plugin_dir() {
    let config = AgentConfig::default().with_plugin_dir("/custom/plugin");
    assert_eq!(config.plugin_dir, Some(PathBuf::from("/custom/plugin")));
}

#[test]
fn test_agent_config_with_agent() {
    let config = AgentConfig::default().with_agent("worker");
    assert_eq!(config.agent, Some("worker".to_string()));
}

// ResponseChunk tests
#[test]
fn test_response_chunk_new() {
    let chunk = ResponseChunk::new("partial");
    assert_eq!(chunk.content, "partial");
    assert!(!chunk.is_final);
}

#[test]
fn test_response_chunk_final() {
    let chunk = ResponseChunk::final_chunk("done");
    assert_eq!(chunk.content, "done");
    assert!(chunk.is_final);
}
