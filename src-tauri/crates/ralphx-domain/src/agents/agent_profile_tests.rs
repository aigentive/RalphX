use super::*;

// ProfileRole tests
#[test]
fn test_profile_role_display() {
    assert_eq!(ProfileRole::Worker.to_string(), "worker");
    assert_eq!(ProfileRole::Reviewer.to_string(), "reviewer");
    assert_eq!(ProfileRole::Supervisor.to_string(), "supervisor");
    assert_eq!(ProfileRole::Orchestrator.to_string(), "orchestrator");
    assert_eq!(ProfileRole::Researcher.to_string(), "researcher");
}

#[test]
fn test_profile_role_from_str() {
    assert_eq!(
        "worker".parse::<ProfileRole>().unwrap(),
        ProfileRole::Worker
    );
    assert_eq!(
        "reviewer".parse::<ProfileRole>().unwrap(),
        ProfileRole::Reviewer
    );
    assert_eq!(
        "supervisor".parse::<ProfileRole>().unwrap(),
        ProfileRole::Supervisor
    );
    assert_eq!(
        "orchestrator".parse::<ProfileRole>().unwrap(),
        ProfileRole::Orchestrator
    );
    assert_eq!(
        "researcher".parse::<ProfileRole>().unwrap(),
        ProfileRole::Researcher
    );
}

#[test]
fn test_profile_role_from_str_invalid() {
    assert!("invalid".parse::<ProfileRole>().is_err());
}

#[test]
fn test_profile_role_serialize() {
    let json = serde_json::to_string(&ProfileRole::Worker).unwrap();
    assert_eq!(json, "\"worker\"");
}

#[test]
fn test_profile_role_deserialize() {
    let role: ProfileRole = serde_json::from_str("\"orchestrator\"").unwrap();
    assert_eq!(role, ProfileRole::Orchestrator);
}

// Model tests
#[test]
fn test_model_display() {
    assert_eq!(Model::Opus.to_string(), "opus");
    assert_eq!(Model::Sonnet.to_string(), "sonnet");
    assert_eq!(Model::Haiku.to_string(), "haiku");
}

#[test]
fn test_model_id() {
    assert_eq!(Model::Opus.model_id(), "claude-opus-4-5-20251101");
    assert_eq!(Model::Sonnet.model_id(), "claude-sonnet-4-6");
    assert_eq!(Model::Haiku.model_id(), "claude-haiku-4-5-20251001");
}

#[test]
fn test_model_serialize() {
    let json = serde_json::to_string(&Model::Sonnet).unwrap();
    assert_eq!(json, "\"sonnet\"");
}

#[test]
fn test_model_deserialize() {
    let model: Model = serde_json::from_str("\"opus\"").unwrap();
    assert_eq!(model, Model::Opus);
}

// PermissionMode tests
#[test]
fn test_permission_mode_default() {
    assert_eq!(PermissionMode::default(), PermissionMode::Default);
}

#[test]
fn test_permission_mode_serialize() {
    let json = serde_json::to_string(&PermissionMode::AcceptEdits).unwrap();
    assert_eq!(json, "\"acceptEdits\"");
}

// AutonomyLevel tests
#[test]
fn test_autonomy_level_default() {
    assert_eq!(AutonomyLevel::default(), AutonomyLevel::Supervised);
}

#[test]
fn test_autonomy_level_serialize() {
    let json = serde_json::to_string(&AutonomyLevel::FullyAutonomous).unwrap();
    assert_eq!(json, "\"fully_autonomous\"");
}

// ClaudeCodeConfig tests
#[test]
fn test_claude_code_config_new() {
    let config = ClaudeCodeConfig::new("worker");
    assert_eq!(config.agent, "worker");
    assert!(config.skills.is_empty());
    assert!(config.hooks.is_none());
    assert!(config.mcp_servers.is_empty());
}

#[test]
fn test_claude_code_config_with_skills() {
    let config = ClaudeCodeConfig::new("worker")
        .with_skills(vec!["skill1".to_string(), "skill2".to_string()]);
    assert_eq!(config.skills.len(), 2);
}

// ExecutionConfig tests
#[test]
fn test_execution_config_default() {
    let config = ExecutionConfig::default();
    assert_eq!(config.model, Model::Sonnet);
    assert_eq!(config.max_iterations, 30);
    assert_eq!(config.timeout_minutes, 30);
    assert_eq!(config.permission_mode, PermissionMode::Default);
}

#[test]
fn test_execution_config_builder() {
    let config = ExecutionConfig::default()
        .with_model(Model::Opus)
        .with_max_iterations(50)
        .with_timeout(60)
        .with_permission_mode(PermissionMode::BypassPermissions);

    assert_eq!(config.model, Model::Opus);
    assert_eq!(config.max_iterations, 50);
    assert_eq!(config.timeout_minutes, 60);
    assert_eq!(config.permission_mode, PermissionMode::BypassPermissions);
}

// IoConfig tests
#[test]
fn test_io_config_default() {
    let config = IoConfig::default();
    assert!(config.input_artifact_types.is_empty());
    assert!(config.output_artifact_types.is_empty());
}

#[test]
fn test_io_config_builder() {
    let config = IoConfig::new()
        .with_inputs(vec!["prd".to_string()])
        .with_outputs(vec!["code".to_string(), "tests".to_string()]);

    assert_eq!(config.input_artifact_types.len(), 1);
    assert_eq!(config.output_artifact_types.len(), 2);
}

// BehaviorConfig tests
#[test]
fn test_behavior_config_default() {
    let config = BehaviorConfig::default();
    assert!(!config.can_spawn_sub_agents);
    assert!(!config.auto_commit);
    assert_eq!(config.autonomy_level, AutonomyLevel::Supervised);
}

#[test]
fn test_behavior_config_builder() {
    let config = BehaviorConfig::default()
        .with_sub_agents(true)
        .with_auto_commit(true)
        .with_autonomy(AutonomyLevel::FullyAutonomous);

    assert!(config.can_spawn_sub_agents);
    assert!(config.auto_commit);
    assert_eq!(config.autonomy_level, AutonomyLevel::FullyAutonomous);
}

// AgentProfile tests
#[test]
fn test_agent_profile_new() {
    let profile = AgentProfile::new(
        "test",
        "Test Agent",
        "A test agent",
        ProfileRole::Worker,
        "test",
    );

    assert_eq!(profile.id, "test");
    assert_eq!(profile.name, "Test Agent");
    assert_eq!(profile.description, "A test agent");
    assert_eq!(profile.role, ProfileRole::Worker);
    assert_eq!(profile.claude_code.agent, "test");
}

#[test]
fn test_agent_profile_worker() {
    let profile = AgentProfile::worker();
    assert_eq!(profile.id, "worker");
    assert_eq!(profile.role, ProfileRole::Worker);
    assert_eq!(profile.execution.model, Model::Sonnet);
    assert_eq!(profile.execution.max_iterations, 30);
    assert!(profile.behavior.auto_commit);
    assert_eq!(profile.claude_code.skills.len(), 3);
}

#[test]
fn test_agent_profile_reviewer() {
    let profile = AgentProfile::reviewer();
    assert_eq!(profile.id, "reviewer");
    assert_eq!(profile.role, ProfileRole::Reviewer);
    assert_eq!(profile.execution.model, Model::Sonnet);
    assert_eq!(profile.execution.max_iterations, 10);
}

#[test]
fn test_agent_profile_orchestrator() {
    let profile = AgentProfile::orchestrator();
    assert_eq!(profile.id, "orchestrator");
    assert_eq!(profile.role, ProfileRole::Orchestrator);
    assert_eq!(profile.execution.model, Model::Opus);
    assert_eq!(profile.execution.max_iterations, 50);
    assert!(profile.behavior.can_spawn_sub_agents);
}

#[test]
fn test_agent_profile_deep_researcher() {
    let profile = AgentProfile::deep_researcher();
    assert_eq!(profile.id, "deep-researcher");
    assert_eq!(profile.role, ProfileRole::Researcher);
    assert_eq!(profile.execution.model, Model::Opus);
    assert_eq!(profile.execution.max_iterations, 200);
}

#[test]
fn test_builtin_profiles() {
    let profiles = AgentProfile::builtin_profiles();
    assert_eq!(profiles.len(), 4);
    assert_eq!(profiles[0].id, "worker");
    assert_eq!(profiles[1].id, "reviewer");
    assert_eq!(profiles[2].id, "orchestrator");
    assert_eq!(profiles[3].id, "deep-researcher");
}

#[test]
fn test_agent_profile_serialize() {
    let profile = AgentProfile::worker();
    let json = serde_json::to_string(&profile).unwrap();
    assert!(json.contains("\"id\":\"worker\""));
    assert!(json.contains("\"role\":\"worker\""));
}

#[test]
fn test_agent_profile_deserialize() {
    let json = r#"{
        "id": "test",
        "name": "Test",
        "description": "Test agent",
        "role": "worker",
        "claudeCode": {
            "agent": "test",
            "skills": [],
            "mcpServers": []
        },
        "execution": {
            "model": "sonnet",
            "maxIterations": 30,
            "timeoutMinutes": 30,
            "permissionMode": "default"
        },
        "io": {
            "inputArtifactTypes": [],
            "outputArtifactTypes": []
        },
        "behavior": {
            "canSpawnSubAgents": false,
            "autoCommit": false,
            "autonomyLevel": "supervised"
        }
    }"#;

    let profile: AgentProfile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.id, "test");
    assert_eq!(profile.role, ProfileRole::Worker);
    assert_eq!(profile.execution.model, Model::Sonnet);
}

#[test]
fn test_agent_profile_roundtrip() {
    let original = AgentProfile::orchestrator();
    let json = serde_json::to_string(&original).unwrap();
    let restored: AgentProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(original, restored);
}
