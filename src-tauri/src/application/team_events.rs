// Team event emission helpers
//
// Emits team:* events via Tauri AppHandle at the appropriate lifecycle points.
// These functions wrap TeamStateTracker operations with event emission.

use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime};

use super::chat_service::events;
use super::chat_service::{
    TeamCostUpdatePayload, TeamCreatedPayload, TeamDisbandedPayload, TeamMessagePayload,
    TeamTeammateIdlePayload, TeamTeammateShutdownPayload, TeamTeammateSpawnedPayload,
};
use super::team_state_tracker::{TeamMessage, TeamMessageType, TeammateStatus};

/// Emit a team:created event
pub fn emit_team_created<R: Runtime>(
    app_handle: &AppHandle<R>,
    team_name: &str,
    context_id: &str,
    context_type: &str,
) {
    let _ = app_handle.emit(
        events::TEAM_CREATED,
        TeamCreatedPayload {
            team_name: team_name.to_string(),
            context_id: context_id.to_string(),
            context_type: context_type.to_string(),
        },
    );
}

/// Emit a team:teammate_spawned event
pub fn emit_teammate_spawned<R: Runtime>(
    app_handle: &AppHandle<R>,
    team_name: &str,
    teammate_name: &str,
    color: &str,
    model: &str,
    role: &str,
    context_type: &str,
    context_id: &str,
    conversation_id: Option<&str>,
) {
    let _ = app_handle.emit(
        events::TEAM_TEAMMATE_SPAWNED,
        TeamTeammateSpawnedPayload {
            team_name: team_name.to_string(),
            teammate_name: teammate_name.to_string(),
            color: color.to_string(),
            model: model.to_string(),
            role: role.to_string(),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
            conversation_id: conversation_id.map(|s| s.to_string()),
        },
    );
}

/// Emit a team:teammate_idle event
pub fn emit_teammate_idle<R: Runtime>(
    app_handle: &AppHandle<R>,
    team_name: &str,
    teammate_name: &str,
    context_type: &str,
    context_id: &str,
) {
    let _ = app_handle.emit(
        events::TEAM_TEAMMATE_IDLE,
        TeamTeammateIdlePayload {
            team_name: team_name.to_string(),
            teammate_name: teammate_name.to_string(),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
        },
    );
}

/// Emit a team:teammate_shutdown event
pub fn emit_teammate_shutdown<R: Runtime>(
    app_handle: &AppHandle<R>,
    team_name: &str,
    teammate_name: &str,
    context_type: &str,
    context_id: &str,
) {
    let _ = app_handle.emit(
        events::TEAM_TEAMMATE_SHUTDOWN,
        TeamTeammateShutdownPayload {
            team_name: team_name.to_string(),
            teammate_name: teammate_name.to_string(),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
        },
    );
}

/// Emit a team:message event
pub fn emit_team_message<R: Runtime>(
    app_handle: &AppHandle<R>,
    message: &TeamMessage,
    context_type: &str,
    context_id: &str,
) {
    let _ = app_handle.emit(
        events::TEAM_MESSAGE,
        TeamMessagePayload {
            team_name: message.team_name.clone(),
            message_id: message.id.clone(),
            sender: message.sender.clone(),
            recipient: message.recipient.clone(),
            content: message.content.clone(),
            message_type: match message.message_type {
                TeamMessageType::UserMessage => "user_message".to_string(),
                TeamMessageType::TeammateMessage => "teammate_message".to_string(),
                TeamMessageType::Broadcast => "broadcast".to_string(),
                TeamMessageType::System => "system".to_string(),
            },
            timestamp: message.timestamp.to_rfc3339(),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
        },
    );
}

/// Emit a team:disbanded event
pub fn emit_team_disbanded<R: Runtime>(
    app_handle: &AppHandle<R>,
    team_name: &str,
    context_type: &str,
    context_id: &str,
) {
    let _ = app_handle.emit(
        events::TEAM_DISBANDED,
        TeamDisbandedPayload {
            team_name: team_name.to_string(),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
        },
    );
}

/// Emit a team:cost_update event
pub fn emit_team_cost_update<R: Runtime>(
    app_handle: &AppHandle<R>,
    team_name: &str,
    teammate_name: &str,
    input_tokens: u64,
    output_tokens: u64,
    estimated_usd: f64,
    context_type: &str,
    context_id: &str,
) {
    let _ = app_handle.emit(
        events::TEAM_COST_UPDATE,
        TeamCostUpdatePayload {
            team_name: team_name.to_string(),
            teammate_name: teammate_name.to_string(),
            input_tokens,
            output_tokens,
            estimated_usd,
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
        },
    );
}

/// Emit a team event based on teammate status change
pub fn emit_teammate_status_change<R: Runtime>(
    app_handle: &AppHandle<R>,
    team_name: &str,
    teammate_name: &str,
    status: TeammateStatus,
    context_type: &str,
    context_id: &str,
) {
    match status {
        TeammateStatus::Idle => emit_teammate_idle(
            app_handle,
            team_name,
            teammate_name,
            context_type,
            context_id,
        ),
        TeammateStatus::Shutdown | TeammateStatus::Failed => {
            emit_teammate_shutdown(
                app_handle,
                team_name,
                teammate_name,
                context_type,
                context_id,
            );
        }
        TeammateStatus::Running => {
            let _ = app_handle.emit(
                "agent:run_started",
                json!({
                    "teammate_name": teammate_name,
                    "context_type": context_type,
                    "context_id": context_id,
                }),
            );
        }
        TeammateStatus::Completed => {
            let _ = app_handle.emit(
                "agent:run_completed",
                json!({
                    "teammate_name": teammate_name,
                    "context_type": context_type,
                    "context_id": context_id,
                }),
            );
        }
        // Spawning has its own dedicated emission path
        TeammateStatus::Spawning => {}
    }
}

#[cfg(test)]
#[path = "team_events_tests.rs"]
mod tests;
