use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};

use super::learned_skills_export::*;
use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    Project, ProjectId, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus,
};
use crate::http_server::project_scope::ProjectScope;

fn test_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState::new_test(app_state)
}

fn temp_project_dir() -> tempfile::TempDir {
    let cwd = std::env::current_dir().expect("current dir");
    tempfile::tempdir_in(cwd).expect("temp project dir")
}

fn approved_skill(project_id: ProjectId) -> ProjectSkill {
    let now = chrono::Utc::now();
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id,
        title: "Review repeat failures".to_string(),
        bucket: "review".to_string(),
        stage: "review".to_string(),
        status: ProjectSkillLifecycleStatus::Approved,
        pinned: false,
        archived: false,
        scope_paths: Vec::new(),
        compact_guidance: "Check repeated review failures before approving.".to_string(),
        body_markdown: "Detailed guidance".to_string(),
        predicted_effect: Some("Reduces repeated review changes.".to_string()),
        provenance_json: serde_json::json!({ "test": true }),
        companion_of_skill_id: None,
        version: 1,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn preview_project_skill_export_lists_approved_skill_files() {
    let project_dir = temp_project_dir();
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-export".to_string());
    let mut project = Project::new(
        "Export Test".to_string(),
        project_dir.path().to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    app_state.project_repo.create(project).await.unwrap();
    app_state
        .project_skill_repo
        .create(approved_skill(project_id.clone()))
        .await
        .unwrap();

    let response = preview_project_skill_export(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(PreviewProjectSkillExportRequest {
            project_id: "project-export".to_string(),
        }),
    )
    .await
    .unwrap()
    .0;

    // One skill exported into both provider roots (.claude/skills + .agents/skills).
    assert_eq!(response.count, 2);
    assert!(response
        .files
        .iter()
        .any(|file| file.relative_path.starts_with(".claude/skills/")));
    assert!(response
        .files
        .iter()
        .any(|file| file.relative_path.starts_with(".agents/skills/")));
    assert!(response.files.iter().all(|file| file.will_write));
}

#[tokio::test]
async fn project_skill_settings_default_to_export_disabled() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-export".to_string());

    let response = get_project_skill_settings(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(GetProjectSkillSettingsRequest {
            project_id: "project-export".to_string(),
        }),
    )
    .await
    .unwrap()
    .0;

    assert!(response.enabled);
    assert_eq!(response.injection_max_skills, 4);
    assert!(!response.export_enabled);
}

#[tokio::test]
async fn update_project_skill_settings_persists_export_opt_in() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-export".to_string());

    let response = update_project_skill_settings(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(UpdateProjectSkillSettingsRequest {
            project_id: "project-export".to_string(),
            enabled: None,
            auto_inject: None,
            auto_distill: None,
            injection_max_skills: None,
            injection_max_chars: None,
            injection_guidance_max_chars: None,
            report_min_outcomes: None,
            verification_corpus_gate: None,
            export_enabled: Some(true),
        }),
    )
    .await
    .unwrap()
    .0;

    assert!(response.export_enabled);
    assert!(response.enabled);
    assert_eq!(response.injection_max_chars, 6_000);
    let loaded = app_state
        .project_skill_settings_repo
        .get_for_project(&project_id)
        .await
        .unwrap()
        .expect("saved settings");
    assert!(loaded.export_enabled);
    assert!(loaded.enabled);
    assert_eq!(loaded.injection_max_chars, 6_000);
}

#[tokio::test]
async fn update_project_skill_settings_rejects_empty_patch_without_write() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-empty-settings".to_string());

    let error = update_project_skill_settings(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(UpdateProjectSkillSettingsRequest {
            project_id: project_id.as_str().to_string(),
            enabled: None,
            auto_inject: None,
            auto_distill: None,
            injection_max_skills: None,
            injection_max_chars: None,
            injection_guidance_max_chars: None,
            report_min_outcomes: None,
            verification_corpus_gate: None,
            export_enabled: None,
        }),
    )
    .await
    .expect_err("empty settings patch must fail");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(app_state
        .project_skill_settings_repo
        .get_for_project(&project_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn apply_project_skill_export_requires_explicit_confirmation() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-export".to_string());

    let error = apply_project_skill_export(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(ApplyProjectSkillExportRequest {
            project_id: "project-export".to_string(),
            confirm_export: false,
        }),
    )
    .await
    .expect_err("export apply should require confirmation");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}
