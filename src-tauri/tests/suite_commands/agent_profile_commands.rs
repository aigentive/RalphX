use ralphx_lib::application::AppState;
use ralphx_lib::commands::agent_profile_commands::{
    get_agent_profile, get_agent_profiles_by_role, get_builtin_agent_profiles,
    get_custom_agent_profiles, list_agent_profiles, seed_builtin_profiles, AgentProfileResponse,
};
use ralphx_lib::domain::agents::{AgentProfile, ProfileRole};
use ralphx_lib::domain::repositories::AgentProfileId;
use tauri::Manager;

fn setup_test_state() -> AppState {
    AppState::new_test()
}

fn agent_profile_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

#[tokio::test]
async fn test_list_agent_profiles_empty() {
    let state = setup_test_state();

    let profiles = state.agent_profile_repo.get_all().await.unwrap();
    assert!(profiles.is_empty());
}

#[tokio::test]
async fn list_agent_profiles_command_maps_all_profiles() {
    let app = agent_profile_command_app();

    app.state::<AppState>()
        .agent_profile_repo
        .seed_builtin_profiles()
        .await
        .expect("builtins seed");

    let profiles = list_agent_profiles(app.state::<AppState>())
        .await
        .expect("profiles list");

    assert_eq!(profiles.len(), 4);
    assert!(profiles.iter().any(|profile| profile.id == "worker"));
    assert!(profiles.iter().any(|profile| profile.id == "reviewer"));
}

#[tokio::test]
async fn test_seed_and_list_builtin_profiles() {
    let state = setup_test_state();

    state
        .agent_profile_repo
        .seed_builtin_profiles()
        .await
        .unwrap();

    let profiles = state.agent_profile_repo.get_all().await.unwrap();
    assert_eq!(profiles.len(), 4);
}

#[tokio::test]
async fn seed_builtin_profiles_command_is_idempotent() {
    let app = agent_profile_command_app();

    seed_builtin_profiles(app.state::<AppState>())
        .await
        .expect("first seed succeeds");
    seed_builtin_profiles(app.state::<AppState>())
        .await
        .expect("second seed succeeds");

    let profiles = list_agent_profiles(app.state::<AppState>())
        .await
        .expect("profiles list");
    assert_eq!(profiles.len(), 4);
}

#[tokio::test]
async fn test_get_agent_profile_by_id() {
    let state = setup_test_state();

    state
        .agent_profile_repo
        .seed_builtin_profiles()
        .await
        .unwrap();

    let profile_id = AgentProfileId::from_string("worker");
    let profile = state
        .agent_profile_repo
        .get_by_id(&profile_id)
        .await
        .unwrap();
    assert!(profile.is_some());
    assert_eq!(profile.unwrap().name, "Worker");
}

#[tokio::test]
async fn get_agent_profile_command_returns_optional_response() {
    let app = agent_profile_command_app();

    let missing = get_agent_profile("missing-profile".to_string(), app.state::<AppState>())
        .await
        .expect("missing profile should not error");
    assert!(missing.is_none());

    seed_builtin_profiles(app.state::<AppState>())
        .await
        .expect("builtins seed");

    let worker = get_agent_profile("worker".to_string(), app.state::<AppState>())
        .await
        .expect("worker lookup succeeds")
        .expect("worker profile exists");

    assert_eq!(worker.id, "worker");
    assert_eq!(worker.name, "Worker");
    assert_eq!(worker.role, "worker");
    assert_eq!(worker.claude_code.agent, "worker");
    assert_eq!(worker.execution.permission_mode, "acceptedits");
    assert!(worker.behavior.auto_commit);
}

#[tokio::test]
async fn test_get_agent_profiles_by_role() {
    let state = setup_test_state();

    state
        .agent_profile_repo
        .seed_builtin_profiles()
        .await
        .unwrap();

    let workers = state
        .agent_profile_repo
        .get_by_role(ProfileRole::Worker)
        .await
        .unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].role, ProfileRole::Worker);
}

#[tokio::test]
async fn get_agent_profiles_by_role_command_filters_case_insensitively_and_rejects_invalid_role() {
    let app = agent_profile_command_app();

    seed_builtin_profiles(app.state::<AppState>())
        .await
        .expect("builtins seed");

    let workers = get_agent_profiles_by_role("WoRkEr".to_string(), app.state::<AppState>())
        .await
        .expect("worker profiles load");
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].id, "worker");
    assert_eq!(workers[0].role, "worker");

    let researchers = get_agent_profiles_by_role("researcher".to_string(), app.state::<AppState>())
        .await
        .expect("researcher profiles load");
    assert_eq!(researchers.len(), 1);
    assert_eq!(researchers[0].id, "deep-researcher");

    let error = get_agent_profiles_by_role("invalid".to_string(), app.state::<AppState>())
        .await
        .expect_err("invalid role should error");
    assert!(error.contains("Invalid role: invalid"));
}

#[tokio::test]
async fn test_get_builtin_profiles() {
    let state = setup_test_state();

    state
        .agent_profile_repo
        .seed_builtin_profiles()
        .await
        .unwrap();

    let builtin = state.agent_profile_repo.get_builtin().await.unwrap();
    assert_eq!(builtin.len(), 4);
}

#[tokio::test]
async fn builtin_and_custom_profile_commands_partition_profiles() {
    let app = agent_profile_command_app();

    seed_builtin_profiles(app.state::<AppState>())
        .await
        .expect("builtins seed");

    let mut custom = AgentProfile::worker();
    custom.id = "custom-worker".to_string();
    custom.name = "Custom Worker".to_string();
    custom.description = "Custom command profile".to_string();
    app.state::<AppState>()
        .agent_profile_repo
        .create(
            &AgentProfileId::from_string("custom-worker"),
            &custom,
            false,
        )
        .await
        .expect("custom profile creates");

    let builtin = get_builtin_agent_profiles(app.state::<AppState>())
        .await
        .expect("builtin profiles load");
    let custom_profiles = get_custom_agent_profiles(app.state::<AppState>())
        .await
        .expect("custom profiles load");

    assert_eq!(builtin.len(), 4);
    assert!(builtin.iter().all(|profile| profile.id != "custom-worker"));
    assert_eq!(custom_profiles.len(), 1);
    assert_eq!(custom_profiles[0].id, "custom-worker");
    assert_eq!(custom_profiles[0].name, "Custom Worker");
}

#[tokio::test]
async fn test_agent_profile_response_serialization() {
    let profile = AgentProfile::worker();
    let response = AgentProfileResponse::from(profile);

    assert_eq!(response.name, "Worker");
    assert_eq!(response.role, "worker");
    assert_eq!(response.execution.model, "sonnet");

    // Verify it serializes to JSON
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"name\":\"Worker\""));
    assert!(json.contains("\"role\":\"worker\""));
}

#[tokio::test]
async fn test_all_builtin_profiles_have_unique_ids() {
    let state = setup_test_state();

    state
        .agent_profile_repo
        .seed_builtin_profiles()
        .await
        .unwrap();

    let profiles = state.agent_profile_repo.get_all().await.unwrap();
    let ids: Vec<_> = profiles.iter().map(|p| &p.id).collect();
    let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique_ids.len());
}
