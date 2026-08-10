use tauri::Manager;

use super::repository_settings_commands::{
    get_repository_settings, update_repository_settings, UpdateRepositorySettingsInput,
};
use crate::infrastructure::subprocess_env_policy;
use crate::AppState;

struct GithubTokenPolicyReset;

impl Drop for GithubTokenPolicyReset {
    fn drop(&mut self) {
        subprocess_env_policy::set_remove_inherited_github_cli_tokens(true);
    }
}

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build repository settings test app")
}

#[tokio::test]
async fn repository_settings_default_to_removing_inherited_github_cli_tokens() {
    let app = test_app();

    let settings = get_repository_settings(app.state::<AppState>())
        .await
        .expect("load repository settings");

    assert!(settings.remove_inherited_github_cli_tokens);
}

#[tokio::test]
async fn repository_settings_opt_out_persists_and_updates_the_live_spawn_policy() {
    let _reset = GithubTokenPolicyReset;
    subprocess_env_policy::set_remove_inherited_github_cli_tokens(true);
    let app = test_app();

    let settings = update_repository_settings(
        UpdateRepositorySettingsInput {
            remove_inherited_github_cli_tokens: false,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("disable inherited GitHub CLI token removal");

    assert!(!settings.remove_inherited_github_cli_tokens);
    assert!(
        !app.state::<AppState>()
            .app_state_repo
            .get()
            .await
            .unwrap()
            .remove_inherited_github_cli_tokens
    );
    assert!(!subprocess_env_policy::remove_inherited_github_cli_tokens());
}
