use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    Json,
};

use super::learned_skill_pipeline::{
    patch_project_skill, retire_project_skill, upsert_project_skill,
};
use crate::application::AppState;
use crate::domain::entities::{ProjectId, ProjectSkillLifecycleStatus};
use crate::domain::repositories::ProjectSkillListOptions;
use crate::http_server::project_scope::ProjectScope;
use crate::http_server::types::{
    HttpServerState, PatchProjectSkillRequest, RetireProjectSkillRequest,
    UpsertProjectSkillRequest,
};

fn state() -> (Arc<AppState>, HttpServerState) {
    let app_state = Arc::new(AppState::new_test());
    let http_state = HttpServerState::new_test(Arc::clone(&app_state));
    (app_state, http_state)
}

fn headers(project_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("x-ralphx-agent-name", "ralphx-memory-capture"),
        ("x-ralphx-pipeline-role", "memory_capture"),
        ("x-ralphx-project-id", project_id),
        ("x-ralphx-context-type", "project"),
        ("x-ralphx-context-id", project_id),
        ("x-ralphx-conversation-id", "conversation-1"),
        ("x-ralphx-agent-run-id", "run-1"),
    ] {
        headers.insert(
            name,
            HeaderValue::from_str(value).expect("valid runtime header"),
        );
    }
    headers
}

fn upsert_request(project_id: &str, title: &str) -> UpsertProjectSkillRequest {
    UpsertProjectSkillRequest {
        project_id: project_id.to_string(),
        title: title.to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        scope_paths: vec!["src-tauri/src/domain/**".to_string()],
        compact_guidance: "Keep one transactional project-skill writer.".to_string(),
        body_markdown: "## Procedure\n\n1. Resolve before mutation.".to_string(),
        predicted_effect: "Avoids duplicate staged procedures.".to_string(),
    }
}

#[tokio::test]
async fn pipeline_handlers_require_trusted_runtime_authority_without_mutation() {
    let (app_state, http_state) = state();
    let project_id = ProjectId::from_string("project-1".to_string());
    let error = upsert_project_skill(
        State(http_state),
        ProjectScope(Some(vec![project_id.clone()])),
        HeaderMap::new(),
        Json(upsert_request(project_id.as_str(), "Missing authority")),
    )
    .await
    .expect_err("missing authority must fail");

    assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    assert!(app_state
        .project_skill_repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .expect("rows")
        .is_empty());
}

#[tokio::test]
async fn pipeline_handlers_run_scoped_upsert_patch_and_retire_flow() {
    let (app_state, http_state) = state();
    let project_id = ProjectId::from_string("project-1".to_string());
    let scope = ProjectScope(Some(vec![project_id.clone()]));
    let created = upsert_project_skill(
        State(http_state.clone()),
        scope.clone(),
        headers(project_id.as_str()),
        Json(upsert_request(project_id.as_str(), "Handler flow")),
    )
    .await
    .expect("upsert")
    .0;
    assert_eq!(created.outcome, "create_new");
    assert_eq!(created.skill.pipeline_role.as_deref(), Some("memory_capture"));

    let mut patch = upsert_request(project_id.as_str(), "Handler flow");
    patch.body_markdown.push_str("\n2. Persist one version.");
    let patched = patch_project_skill(
        State(http_state.clone()),
        scope.clone(),
        headers(project_id.as_str()),
        Json(PatchProjectSkillRequest {
            project_skill_id: created.skill.id.clone(),
            project_id: patch.project_id,
            title: patch.title,
            bucket: patch.bucket,
            stage: patch.stage,
            scope_paths: patch.scope_paths,
            compact_guidance: patch.compact_guidance,
            body_markdown: patch.body_markdown,
            predicted_effect: patch.predicted_effect,
        }),
    )
    .await
    .expect("patch")
    .0;
    assert_eq!(patched.outcome, "patch_existing");

    let retired = retire_project_skill(
        State(http_state),
        scope,
        headers(project_id.as_str()),
        Json(RetireProjectSkillRequest {
            project_id: project_id.as_str().to_string(),
            project_skill_id: created.skill.id.clone(),
        }),
    )
    .await
    .expect("retire")
    .0;
    assert_eq!(retired.outcome, "retired");
    assert_eq!(retired.skill.status, "retired");
    assert_eq!(
        app_state
            .project_skill_repo
            .list_versions(&crate::domain::entities::ProjectSkillId::from_string(
                created.skill.id,
            ))
            .await
            .expect("versions")
            .len(),
        2
    );
}

#[tokio::test]
async fn pipeline_handlers_reject_runtime_project_mismatch_before_writing() {
    let (app_state, http_state) = state();
    let project_id = ProjectId::from_string("project-1".to_string());
    let error = upsert_project_skill(
        State(http_state),
        ProjectScope(Some(vec![project_id.clone()])),
        headers("other-project"),
        Json(upsert_request(project_id.as_str(), "Wrong runtime project")),
    )
    .await
    .expect_err("runtime mismatch");

    assert_eq!(error.status, StatusCode::FORBIDDEN);
    assert!(app_state
        .project_skill_repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .expect("rows")
        .iter()
        .all(|skill| skill.status != ProjectSkillLifecycleStatus::Staged));
}
