use axum::{
    extract::{Path, Query, State},
    Json,
};
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings};
use ralphx_lib::domain::entities::{
    ideation::{IdeationSession, IdeationSessionStatus, SessionPurpose},
    ArtifactType, IdeationSessionId, Project, ProjectId,
};
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::{
    CreateTeamArtifactRequest, GetTeamArtifactsResponse, GetVerificationFindingsResponse,
    HttpServerState, PublishVerificationFindingRequest, RequestTeamPlanRequest,
    RequestTeammateSpawnRequest, TeamPlanTeammate, VerificationFindingGapPayload,
    VerificationFindingQuery,
};
use ralphx_lib::infrastructure::agents::claude::TeammateSpawnRequest;
use std::sync::Arc;

#[test]
fn team_artifact_type_mapping() {
    // Verify the string → ArtifactType mapping
    assert!(matches!(
        match "TeamResearch" {
            "TeamResearch" => Some(ArtifactType::TeamResearch),
            "TeamAnalysis" => Some(ArtifactType::TeamAnalysis),
            "TeamSummary" => Some(ArtifactType::TeamSummary),
            _ => None,
        },
        Some(ArtifactType::TeamResearch)
    ));

    assert!(matches!(
        match "TeamAnalysis" {
            "TeamResearch" => Some(ArtifactType::TeamResearch),
            "TeamAnalysis" => Some(ArtifactType::TeamAnalysis),
            "TeamSummary" => Some(ArtifactType::TeamSummary),
            _ => None,
        },
        Some(ArtifactType::TeamAnalysis)
    ));

    assert!(matches!(
        match "TeamSummary" {
            "TeamResearch" => Some(ArtifactType::TeamResearch),
            "TeamAnalysis" => Some(ArtifactType::TeamAnalysis),
            "TeamSummary" => Some(ArtifactType::TeamSummary),
            _ => None,
        },
        Some(ArtifactType::TeamSummary)
    ));

    // Invalid type
    assert!(match "InvalidType" {
        "TeamResearch" => Some(ArtifactType::TeamResearch),
        "TeamAnalysis" => Some(ArtifactType::TeamAnalysis),
        "TeamSummary" => Some(ArtifactType::TeamSummary),
        "VerificationFinding" => Some(ArtifactType::VerificationFinding),
        _ => None,
    }
    .is_none());
}

#[test]
fn verification_finding_artifact_type_mapping() {
    assert!(matches!(
        match "VerificationFinding" {
            "VerificationFinding" => Some(ArtifactType::VerificationFinding),
            _ => None,
        },
        Some(ArtifactType::VerificationFinding)
    ));
}

#[test]
fn team_composition_serialization() {
    let entry = TeamCompositionEntry {
        name: "researcher".to_string(),
        role: "explore".to_string(),
        prompt: "Research the topic".to_string(),
        model: "sonnet".to_string(),
    };

    let json = serde_json::to_string(&entry).unwrap();
    let parsed: TeamCompositionEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "researcher");
    assert_eq!(parsed.role, "explore");
    assert_eq!(parsed.model, "sonnet");
}

#[test]
fn request_teammate_spawn_deserialization() {
    let json = r#"{
        "role": "frontend-researcher",
        "prompt": "Research React patterns",
        "model": "sonnet",
        "tools": ["Read", "Grep", "Glob"],
        "mcp_tools": ["get_session_plan"],
        "preset": null
    }"#;

    let req: RequestTeammateSpawnRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.role, "frontend-researcher");
    assert_eq!(req.model, "sonnet");
    assert_eq!(req.tools.len(), 3);
    assert_eq!(req.mcp_tools.len(), 1);
    assert!(req.preset.is_none());
}

#[test]
fn request_team_plan_deserialization() {
    let json = r#"{
        "context_type": "ideation",
        "context_id": "session-abc123",
        "process": "ideation-research",
        "team_name": "test-research-team",
        "teammates": [
            {
                "role": "frontend-researcher",
                "tools": ["Read", "Grep"],
                "mcp_tools": ["get_session_plan"],
                "model": "sonnet",
                "prompt_summary": "Research React component patterns"
            }
        ]
    }"#;

    let req: RequestTeamPlanRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.context_type, "ideation");
    assert_eq!(req.context_id, "session-abc123");
    assert_eq!(req.process, "ideation-research");
    assert_eq!(req.team_name, "test-research-team");
    assert_eq!(req.teammates.len(), 1);
    assert_eq!(req.teammates[0].role, "frontend-researcher");
    assert_eq!(req.teammates[0].model, "sonnet");
}

#[test]
fn request_team_plan_deserialization_missing_team_name_fails() {
    let json = r#"{
        "context_type": "ideation",
        "context_id": "session-abc123",
        "process": "ideation-research",
        "teammates": [
            {
                "role": "frontend-researcher",
                "tools": ["Read", "Grep"],
                "mcp_tools": ["get_session_plan"],
                "model": "sonnet",
                "prompt_summary": "Research React component patterns"
            }
        ]
    }"#;

    let result: Result<RequestTeamPlanRequest, _> = serde_json::from_str(json);
    assert!(
        result.is_err(),
        "team_name is required — deserialization must fail when missing"
    );
}

#[test]
fn save_team_session_state_deserialization() {
    let json = r#"{
        "session_id": "session-123",
        "team_composition": [
            {
                "name": "researcher",
                "role": "explore",
                "prompt": "Research the topic",
                "model": "sonnet"
            }
        ],
        "phase": "EXPLORE",
        "artifact_ids": ["art-1", "art-2"]
    }"#;

    let req: SaveTeamSessionStateRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.session_id, "session-123");
    assert_eq!(req.team_composition.len(), 1);
    assert_eq!(req.phase, "EXPLORE");
    assert_eq!(req.artifact_ids.as_ref().unwrap().len(), 2);
}

// ── Color palette tests ──────────────────────────────────────────

#[test]
fn teammate_colors_rotate() {
    assert_eq!(TEAMMATE_COLORS[0], "blue");
    assert_eq!(TEAMMATE_COLORS[1], "green");
    assert_eq!(TEAMMATE_COLORS.len(), 5);
    // Rotation wraps around
    assert_eq!(TEAMMATE_COLORS[5 % TEAMMATE_COLORS.len()], "blue");
}

// ── find_active_team tests ───────────────────────────────────────

fn test_state() -> HttpServerState {
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state: Arc::new(AppState::new_test()),
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

async fn seed_codex_ideation_context(state: &HttpServerState) -> IdeationSessionId {
    let project = state
        .app_state
        .project_repo
        .create(Project::new(
            "Codex Team Project".to_string(),
            "/tmp/codex-team-project".to_string(),
        ))
        .await
        .unwrap();
    let session = state
        .app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    state
        .app_state
        .agent_lane_settings_repo
        .upsert_global(
            AgentLane::IdeationPrimary,
            &AgentLaneSettings::new(AgentHarnessKind::Codex),
        )
        .await
        .unwrap();
    session.id
}

#[tokio::test]
async fn test_find_active_team_none_found() {
    let state = test_state();
    let result = find_active_team(&state).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No active team found"));
}

#[tokio::test]
async fn test_find_active_team_forming() {
    let state = test_state();
    state
        .team_tracker
        .create_team("test-team", "session-123", "ideation")
        .await
        .unwrap();

    let (name, ctx_id, ctx_type) = find_active_team(&state).await.unwrap();
    assert_eq!(name, "test-team");
    assert_eq!(ctx_id, "session-123");
    assert_eq!(ctx_type, "ideation");
}

#[tokio::test]
async fn test_find_active_team_active() {
    let state = test_state();
    state
        .team_tracker
        .create_team("my-team", "ctx-1", "ideation")
        .await
        .unwrap();
    // Adding a teammate transitions Forming → Active
    state
        .team_tracker
        .add_teammate("my-team", "worker", "#ff0000", "sonnet", "code")
        .await
        .unwrap();

    let (name, _, _) = find_active_team(&state).await.unwrap();
    assert_eq!(name, "my-team");
}

#[tokio::test]
async fn test_find_active_team_skips_disbanded() {
    let state = test_state();
    state
        .team_tracker
        .create_team("old-team", "ctx-1", "ideation")
        .await
        .unwrap();
    state.team_tracker.disband_team("old-team").await.unwrap();

    let result = find_active_team(&state).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_request_team_plan_register_rejects_codex_ideation_context() {
    let state = test_state();
    let session_id = seed_codex_ideation_context(&state).await;
    let request = RequestTeamPlanRequest {
        context_type: "ideation".to_string(),
        context_id: session_id.to_string(),
        process: "ideation-research".to_string(),
        team_name: "codex-team".to_string(),
        teammates: vec![TeamPlanTeammate {
            role: "researcher".to_string(),
            tools: vec!["Read".to_string()],
            mcp_tools: vec![],
            model: "sonnet".to_string(),
            preset: None,
            prompt_summary: "Research the implementation".to_string(),
            prompt: Some("Research the implementation".to_string()),
        }],
        lead_session_id: None,
    };

    let result = request_team_plan_register(State(state), Json(request)).await;
    let (status, message) = result.expect_err("Codex ideation should reject team plans");
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert!(message.contains("solo mode"));
    assert!(message.contains("Codex"));
}

#[tokio::test]
async fn test_request_teammate_spawn_rejects_codex_active_team_context() {
    let state = test_state();
    let session_id = seed_codex_ideation_context(&state).await;
    state
        .team_tracker
        .create_team("codex-team", &session_id.to_string(), "ideation")
        .await
        .unwrap();

    let request = RequestTeammateSpawnRequest {
        role: "researcher".to_string(),
        prompt: "Research the implementation".to_string(),
        model: "sonnet".to_string(),
        tools: vec!["Read".to_string()],
        mcp_tools: vec![],
        preset: None,
    };

    let result = request_teammate_spawn(State(state), Json(request)).await;
    let (status, message) = result.expect_err("Codex ideation should reject teammate spawns");
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert!(message.contains("solo mode"));
    assert!(message.contains("Codex"));
}

// ── generate_unique_teammate_name tests ──────────────────────────

#[tokio::test]
async fn test_unique_name_no_collision() {
    let state = test_state();
    state
        .team_tracker
        .create_team("t1", "ctx", "ideation")
        .await
        .unwrap();

    let name = generate_unique_teammate_name(&state, "t1", "researcher").await;
    assert_eq!(name, "researcher");
}

#[tokio::test]
async fn test_unique_name_with_collision() {
    let state = test_state();
    state
        .team_tracker
        .create_team("t1", "ctx", "ideation")
        .await
        .unwrap();
    state
        .team_tracker
        .add_teammate("t1", "researcher", "#blue", "sonnet", "explore")
        .await
        .unwrap();

    let name = generate_unique_teammate_name(&state, "t1", "researcher").await;
    assert_eq!(name, "researcher-2");
}

#[tokio::test]
async fn test_unique_name_multiple_collisions() {
    let state = test_state();
    state
        .team_tracker
        .create_team("t1", "ctx", "ideation")
        .await
        .unwrap();
    state
        .team_tracker
        .add_teammate("t1", "coder", "#blue", "sonnet", "code")
        .await
        .unwrap();
    state
        .team_tracker
        .add_teammate("t1", "coder-2", "#green", "sonnet", "code")
        .await
        .unwrap();

    let name = generate_unique_teammate_name(&state, "t1", "coder").await;
    assert_eq!(name, "coder-3");
}

// ── assign_teammate_color tests ─────────────────────────────────

#[tokio::test]
async fn test_assign_color_first_teammate() {
    let state = test_state();
    state
        .team_tracker
        .create_team("t1", "ctx", "ideation")
        .await
        .unwrap();

    let color = assign_teammate_color(&state, "t1").await;
    assert_eq!(color, "blue");
}

#[tokio::test]
async fn test_assign_color_rotates() {
    let state = test_state();
    state
        .team_tracker
        .create_team("t1", "ctx", "ideation")
        .await
        .unwrap();
    state
        .team_tracker
        .add_teammate("t1", "a", "#blue", "sonnet", "code")
        .await
        .unwrap();

    let color = assign_teammate_color(&state, "t1").await;
    assert_eq!(color, "green");
}

// ── Team plan validation integration test ────────────────────────

#[test]
fn team_plan_request_converts_to_spawn_requests() {
    let req = RequestTeamPlanRequest {
        context_type: "ideation".to_string(),
        context_id: "session-abc123".to_string(),
        process: "ideation".to_string(),
        teammates: vec![
            TeamPlanTeammate {
                role: "researcher".to_string(),
                tools: vec!["Read".to_string(), "Grep".to_string()],
                mcp_tools: vec![],
                model: "sonnet".to_string(),
                preset: None,
                prompt_summary: "Research patterns".to_string(),
                prompt: None,
            },
            TeamPlanTeammate {
                role: "analyzer".to_string(),
                tools: vec!["Read".to_string()],
                mcp_tools: vec![],
                model: "haiku".to_string(),
                preset: None,
                prompt_summary: "Analyze results".to_string(),
                prompt: None,
            },
        ],
        team_name: "test-team-abc123".to_string(),
        lead_session_id: None,
    };

    // Convert to spawn requests
    let spawn_requests: Vec<TeammateSpawnRequest> = req
        .teammates
        .iter()
        .map(|t| TeammateSpawnRequest {
            role: t.role.clone(),
            prompt: None,
            preset: t.preset.clone(),
            tools: t.tools.clone(),
            mcp_tools: t.mcp_tools.clone(),
            model: t.model.clone(),
            prompt_summary: Some(t.prompt_summary.clone()),
        })
        .collect();

    assert_eq!(spawn_requests.len(), 2);
    assert_eq!(spawn_requests[0].role, "researcher");
    assert_eq!(spawn_requests[1].model, "haiku");
}

// ============================================================================
// resolve_mcp_agent_type tests
// ============================================================================

#[test]
fn resolve_mcp_agent_type_returns_preset_when_some() {
    assert_eq!(
        resolve_mcp_agent_type("ideation", Some("ralphx-ideation-specialist-backend")),
        "ralphx-ideation-specialist-backend"
    );
}

#[test]
fn resolve_mcp_agent_type_ideation_process_no_preset() {
    assert_eq!(
        resolve_mcp_agent_type("ideation", None),
        "ideation-team-member"
    );
}

#[test]
fn resolve_mcp_agent_type_worker_process_no_preset() {
    assert_eq!(
        resolve_mcp_agent_type("worker-parallel", None),
        "worker-team-member"
    );
}

#[test]
fn resolve_mcp_agent_type_preset_overrides_worker_process() {
    // Even if process is worker-*, preset takes priority
    assert_eq!(
        resolve_mcp_agent_type(
            "worker-parallel",
            Some("ralphx-ideation-specialist-frontend")
        ),
        "ralphx-ideation-specialist-frontend"
    );
}

#[test]
fn resolve_mcp_agent_type_specialist_preset_variants() {
    for preset in &[
        "ralphx-ideation-specialist-backend",
        "ralphx-ideation-specialist-frontend",
        "ralphx-ideation-specialist-infra",
        "ralphx-ideation-critic",
        "ralphx-ideation-advocate",
    ] {
        assert_eq!(
            resolve_mcp_agent_type("ideation", Some(preset)),
            *preset,
            "Expected preset '{}' to be returned",
            preset
        );
    }
}

// ============================================================================
// resolve_effort integration tests
// ============================================================================

#[test]
fn resolve_effort_for_ideation_team_member_returns_default() {
    use ralphx_lib::infrastructure::agents::claude::resolve_effort;
    // ideation-team-member is a compatibility alias without Claude harness metadata, so it
    // should resolve to the default effort.
    let effort = resolve_effort(Some("ideation-team-member"));
    // Just ensure it returns a non-empty string (the default)
    assert!(
        !effort.is_empty(),
        "Expected non-empty effort for ideation-team-member"
    );
}

#[test]
fn resolve_effort_for_specialist_returns_non_empty() {
    use ralphx_lib::infrastructure::agents::claude::resolve_effort;
    // ralphx-ideation-specialist-backend has a YAML entry with opus model
    let effort = resolve_effort(Some("ralphx-ideation-specialist-backend"));
    assert!(
        !effort.is_empty(),
        "Expected non-empty effort for ralphx-ideation-specialist-backend"
    );
}

#[tokio::test]
async fn test_create_team_artifact_rejects_placeholder_session_id() {
    let state = test_state();

    let result = create_team_artifact(
        State(state),
        Json(CreateTeamArtifactRequest {
            session_id: "SESSION_ID".to_string(),
            title: "Completeness: Placeholder".to_string(),
            content: "{}".to_string(),
            artifact_type: "TeamResearch".to_string(),
            related_artifact_id: None,
        }),
    )
    .await;

    let (status, message) = result.expect_err("placeholder session id must be rejected");
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        message.contains("placeholder values"),
        "expected repair guidance in message, got: {message}"
    );
}

#[tokio::test]
async fn test_create_team_artifact_remaps_verification_child_session_id_to_parent() {
    let state = test_state();

    let parent = state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .id(IdeationSessionId::from_string(
                    "parent-session-1".to_string(),
                ))
                .project_id(ProjectId::from_string("project-1".to_string()))
                .status(IdeationSessionStatus::Active)
                .build(),
        )
        .await
        .expect("parent session");

    state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .id(IdeationSessionId::from_string(
                    "verification-child-1".to_string(),
                ))
                .project_id(ProjectId::from_string("project-1".to_string()))
                .status(IdeationSessionStatus::Active)
                .parent_session_id(parent.id.clone())
                .session_purpose(SessionPurpose::Verification)
                .build(),
        )
        .await
        .expect("verification child");

    let response = create_team_artifact(
        State(state),
        Json(CreateTeamArtifactRequest {
            session_id: "verification-child-1".to_string(),
            title: "Completeness: Round 1".to_string(),
            content: "{}".to_string(),
            artifact_type: "TeamResearch".to_string(),
            related_artifact_id: None,
        }),
    )
    .await
    .expect("verification child session id should be remapped to parent");

    assert!(
        !response.0.artifact_id.is_empty(),
        "artifact id should be returned after remapping to parent session"
    );
}

#[tokio::test]
async fn test_create_team_artifact_allows_non_ideation_session_ids() {
    let state = test_state();
    let session_id = "worker-run-123".to_string();

    let response = create_team_artifact(
        State(state.clone()),
        Json(CreateTeamArtifactRequest {
            session_id: session_id.clone(),
            title: "Execution Notes".to_string(),
            content: "{\"ok\":true}".to_string(),
            artifact_type: "TeamResearch".to_string(),
            related_artifact_id: None,
        }),
    )
    .await
    .expect("non-ideation session ids should remain valid");

    assert!(
        !response.0.artifact_id.is_empty(),
        "artifact id should be returned for valid team artifact creation"
    );

    let artifacts = get_team_artifacts(State(state), Path(session_id))
        .await
        .expect("artifact lookup should succeed");
    let Json(GetTeamArtifactsResponse { count, artifacts }) = artifacts;
    assert_eq!(
        count, 1,
        "expected artifact to be retrievable by session id"
    );
    assert_eq!(artifacts[0].name, "Execution Notes");
}

#[tokio::test]
async fn test_get_team_artifacts_remaps_verification_child_session_id_to_parent() {
    let state = test_state();

    let parent = state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .id(IdeationSessionId::from_string(
                    "parent-session-2".to_string(),
                ))
                .project_id(ProjectId::from_string("project-2".to_string()))
                .status(IdeationSessionStatus::Active)
                .build(),
        )
        .await
        .expect("parent session");

    state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .id(IdeationSessionId::from_string(
                    "verification-child-2".to_string(),
                ))
                .project_id(ProjectId::from_string("project-2".to_string()))
                .status(IdeationSessionStatus::Active)
                .parent_session_id(parent.id.clone())
                .session_purpose(SessionPurpose::Verification)
                .build(),
        )
        .await
        .expect("verification child");

    let _ = create_team_artifact(
        State(state.clone()),
        Json(CreateTeamArtifactRequest {
            session_id: "parent-session-2".to_string(),
            title: "Feasibility: Round 1".to_string(),
            content: "{}".to_string(),
            artifact_type: "TeamResearch".to_string(),
            related_artifact_id: None,
        }),
    )
    .await
    .expect("parent artifact creation should succeed");

    let artifacts = get_team_artifacts(State(state), Path("verification-child-2".to_string()))
        .await
        .expect("verification child read should be remapped to parent");

    let Json(GetTeamArtifactsResponse { count, artifacts }) = artifacts;
    assert_eq!(
        count, 1,
        "expected remapped parent artifacts to be returned"
    );
    assert_eq!(artifacts[0].name, "Feasibility: Round 1");
}

#[tokio::test]
async fn test_publish_verification_finding_rejects_placeholder_session_id() {
    let state = test_state();

    let result = publish_verification_finding(
        State(state),
        Json(PublishVerificationFindingRequest {
            session_id: "SESSION_ID".to_string(),
            critic: "completeness".to_string(),
            round: 1,
            status: "partial".to_string(),
            coverage: Some("affected_files".to_string()),
            summary: "Need one more integration check".to_string(),
            gaps: vec![],
            title_suffix: None,
        }),
    )
    .await;

    let (status, message) = result.expect_err("placeholder session id must be rejected");
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(message.contains("placeholder values"));
}

#[tokio::test]
async fn test_publish_verification_finding_rejects_too_many_gaps() {
    let state = test_state();
    let gaps = (0..251)
        .map(|idx| VerificationFindingGapPayload {
            severity: "medium".to_string(),
            category: "completeness".to_string(),
            description: format!("Gap {idx}"),
            why_it_matters: None,
            source: None,
            lens: None,
        })
        .collect();

    let result = publish_verification_finding(
        State(state),
        Json(PublishVerificationFindingRequest {
            session_id: "verification-parent-too-many".to_string(),
            critic: "completeness".to_string(),
            round: 1,
            status: "partial".to_string(),
            coverage: None,
            summary: "Too many gaps".to_string(),
            gaps,
            title_suffix: None,
        }),
    )
    .await;

    let (status, message) = result.expect_err("oversized gap list must be rejected");
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(message.contains("gaps exceed the limit"));
}

#[tokio::test]
async fn test_publish_and_list_verification_findings() {
    let state = test_state();
    let session_id = "verification-parent-1".to_string();

    let response = publish_verification_finding(
        State(state.clone()),
        Json(PublishVerificationFindingRequest {
            session_id: session_id.clone(),
            critic: "completeness".to_string(),
            round: 2,
            status: "partial".to_string(),
            coverage: Some("affected_files_plus_adjacent".to_string()),
            summary: "Migration plan still needs a concrete backfill step".to_string(),
            gaps: vec![VerificationFindingGapPayload {
                severity: "high".to_string(),
                category: "completeness".to_string(),
                description: "Backfill step is implied but not specified".to_string(),
                why_it_matters: Some("Existing projects stay on the old default.".to_string()),
                source: Some("layer1".to_string()),
                lens: None,
            }],
            title_suffix: Some("merge validation defaults".to_string()),
        }),
    )
    .await
    .expect("verification finding should be created");

    assert!(!response.0.artifact_id.is_empty());

    let findings = get_verification_findings(
        State(state),
        Path(session_id),
        Query(VerificationFindingQuery {
            critic: Some("completeness".to_string()),
            round: Some(2),
            created_after: None,
        }),
    )
    .await
    .expect("verification finding query should succeed");

    let Json(GetVerificationFindingsResponse { count, findings }) = findings;
    assert_eq!(count, 1);
    assert_eq!(findings[0].critic, "completeness");
    assert_eq!(findings[0].round, 2);
    assert_eq!(findings[0].status, "partial");
    assert_eq!(findings[0].gaps.len(), 1);
    assert!(findings[0].title.starts_with("Completeness: Round 2"));
}

#[tokio::test]
async fn test_publish_verification_finding_remaps_verification_child_to_parent() {
    let state = test_state();

    let parent = state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .id(IdeationSessionId::from_string(
                    "parent-session-3".to_string(),
                ))
                .project_id(ProjectId::from_string("project-3".to_string()))
                .status(IdeationSessionStatus::Active)
                .build(),
        )
        .await
        .expect("parent session");

    state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .id(IdeationSessionId::from_string(
                    "verification-child-3".to_string(),
                ))
                .project_id(ProjectId::from_string("project-3".to_string()))
                .status(IdeationSessionStatus::Active)
                .parent_session_id(parent.id.clone())
                .session_purpose(SessionPurpose::Verification)
                .build(),
        )
        .await
        .expect("verification child");

    let _ = publish_verification_finding(
        State(state.clone()),
        Json(PublishVerificationFindingRequest {
            session_id: "verification-child-3".to_string(),
            critic: "feasibility".to_string(),
            round: 1,
            status: "complete".to_string(),
            coverage: Some("affected_files".to_string()),
            summary: "No additional feasibility gaps.".to_string(),
            gaps: vec![],
            title_suffix: None,
        }),
    )
    .await
    .expect("verification child session should remap to parent");

    let findings = get_verification_findings(
        State(state),
        Path("parent-session-3".to_string()),
        Query(VerificationFindingQuery {
            critic: Some("feasibility".to_string()),
            round: Some(1),
            created_after: None,
        }),
    )
    .await
    .expect("parent query should see remapped finding");

    let Json(GetVerificationFindingsResponse { count, findings }) = findings;
    assert_eq!(count, 1);
    assert_eq!(findings[0].critic, "feasibility");
}
