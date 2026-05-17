use ralphx_lib::application::AppState;
use ralphx_lib::commands::release_notes_commands::{
    get_current_release_notes, get_last_seen_release_notes_version, mark_release_notes_seen,
    ReleaseNotesSource,
};
use tauri::test::{mock_builder, MockRuntime};
use tauri::Manager;

fn release_notes_command_app() -> tauri::App<MockRuntime> {
    mock_builder()
        .manage(AppState::new_test())
        .build(tauri::generate_context!())
        .expect("mock app should build")
}

#[tokio::test]
async fn ipc_contract_current_release_notes_reads_current_version_notes() {
    let app = release_notes_command_app();

    let response = get_current_release_notes(app.handle().clone())
        .await
        .expect("current release notes should load");

    let expected_version = env!("CARGO_PKG_VERSION");
    assert_eq!(response.version, expected_version);
    assert!(matches!(
        response.source,
        ReleaseNotesSource::BundledResource | ReleaseNotesSource::DevelopmentCheckout
    ));
    assert!(response
        .body
        .as_deref()
        .is_some_and(|body| body.contains(&format!("# RalphX.app v{expected_version}"))));
}

#[tokio::test]
async fn ipc_contract_last_seen_release_notes_round_trips() {
    let app = release_notes_command_app();

    assert_eq!(
        get_last_seen_release_notes_version(app.state::<AppState>())
            .await
            .expect("last seen version should load"),
        None
    );

    mark_release_notes_seen("v0.9.0".to_string(), app.state::<AppState>())
        .await
        .expect("release notes version should persist");

    assert_eq!(
        get_last_seen_release_notes_version(app.state::<AppState>())
            .await
            .expect("last seen version should reload"),
        Some("0.9.0".to_string())
    );
}

#[tokio::test]
async fn ipc_contract_mark_release_notes_seen_rejects_invalid_versions() {
    let app = release_notes_command_app();

    let error = mark_release_notes_seen("../0.9.0".to_string(), app.state::<AppState>())
        .await
        .expect_err("path-like versions should be rejected");

    assert_eq!(error, "Invalid release notes version");
}
