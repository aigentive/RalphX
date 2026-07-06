// Team HTTP handlers — endpoints for MCP team tools
//
// Provides HTTP endpoints for:
// - POST /api/team/plan/request   — request_team_plan_register (Phase 1: validate + store + emit + return plan_id)
// - GET  /api/team/plan/await/:plan_id — await_team_plan (Phase 2: long-poll 840s until decision)
// - POST /api/team/plan/approve   — approve_team_plan (batch-spawns all teammates from approved plan)
// - POST /api/team/plan/reject    — reject_team_plan
// - GET  /api/team/plan/pending/:context_id — get_pending_plan (frontend reconciliation)
// - POST /api/team/spawn          — request_teammate_spawn (validates, spawns, registers, streams)
// - POST /api/team/artifact       — create_team_artifact
// - POST /api/team/verification_finding — publish_verification_finding
// - GET  /api/team/artifacts/:session_id — get_team_artifacts
// - GET  /api/team/verification-findings/:session_id — get_verification_findings
// - GET  /api/team/session_state/:session_id — get_team_session_state
// - POST /api/team/session_state  — save_team_session_state

use std::path::PathBuf;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use tracing::{error, info, warn};

use super::HttpServerState;
use crate::application::team_state_tracker::{
    PendingTeamPlan, PendingTeammate, PlanDecision, PlanDecisionTeammate, TeammateHandle,
    TeammateStatus,
};
use crate::domain::entities::{
    Artifact, ArtifactBucketId, ArtifactContent, ArtifactId, ArtifactRelation, ArtifactRelationId,
    ArtifactRelationType, ArtifactType, TaskId,
};
use crate::http_server::types::{
    ApproveTeamPlanRequest, ApproveTeamPlanResponse, CreateTeamArtifactRequest,
    CreateTeamArtifactResponse, GetPendingPlanResponse, GetTeamArtifactsResponse,
    GetVerificationFindingsResponse, PublishVerificationFindingRequest,
    PublishVerificationFindingResponse, RejectTeamPlanRequest, RequestTeamPlanRequest,
    RequestTeamPlanResponse, RequestTeammateSpawnRequest, RequestTeammateSpawnResponse,
    SaveTeamSessionStateRequest, SaveTeamSessionStateResponse, SpawnedTeammateInfo,
    TeamArtifactSummary, TeamCompositionEntry, TeamPlanRegisterResponse, TeamSessionStateResponse,
    VerificationFindingGapPayload, VerificationFindingQuery, VerificationFindingSummary,
};
use crate::infrastructure::agents::claude::{
    get_team_constraints, resolve_effort, team_constraints_config, validate_team_plan,
    ClaudeCodeClient, TeammateContext, TeammateSpawnConfig, TeammateSpawnRequest,
};

mod artifacts;
mod plan;
mod session_state;
mod spawn;
mod spawn_execution;
mod spawn_helpers;

pub use self::artifacts::{
    create_team_artifact, get_team_artifacts, get_verification_findings,
    publish_verification_finding,
};
pub use self::plan::{
    approve_team_plan, await_team_plan, get_pending_plan, reject_team_plan,
    request_team_plan_register,
};
pub use self::session_state::{get_team_session_state, save_team_session_state};
pub use self::spawn::request_teammate_spawn;
pub use self::spawn_helpers::{
    assign_teammate_color, find_active_team, generate_unique_teammate_name, resolve_mcp_agent_type,
    TEAMMATE_COLORS,
};

use self::spawn_execution::execute_team_spawn;
use self::spawn_helpers::{
    ensure_team_mode_supported_for_context, load_claude_provider_env_for_teammate_spawn,
    resolve_lead_session_from_config, resolve_teammate_plugin_dir, resolve_teammate_project_id,
    resolve_teammate_working_dir,
};
