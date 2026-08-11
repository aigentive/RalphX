use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::error;

use crate::application::agent_planning_session_titles::sync_linked_planning_session_title_from_conversation;
use crate::application::task_cleanup_service::TaskCleanupService;
use crate::application::{
    spawn_ready_task_scheduler_if_needed, CreateProposalOptions, UpdateProposalOptions,
    UpdateSource,
};
use crate::domain::entities::{ChatConversationId, IdeationSessionId, Priority, TaskProposalId};
use crate::domain::services::{
    emit_external_webhook_event, PresentationKind, WebhookPresentationContext,
};
use crate::http_server::handlers::ideation::stop_verification_children;
use crate::http_server::helpers::{
    archive_proposal_impl, create_proposal_impl, finalize_proposals_impl, parse_category,
    parse_priority, update_proposal_impl,
};
use crate::http_server::types::{
    CreateProposalRequest, DeleteProposalRequest, FinalizeProposalsRequest,
    FinalizeProposalsResponse, HttpServerState, ListProposalsResponse, ProposalDetailResponse,
    ProposalResponse, ProposalSummary, SuccessResponse, UpdateProposalRequest,
    UpdateSessionTitleRequest,
};

use super::{json_error, JsonError};

pub async fn create_task_proposal(
    State(state): State<HttpServerState>,
    Json(req): Json<CreateProposalRequest>,
) -> Result<Json<ProposalResponse>, JsonError> {
    let session_id = IdeationSessionId::from_string(req.session_id.clone());

    // Parse category
    let category = parse_category(&req.category).map_err(|e| {
        error!("Invalid category '{}': {}", req.category, e);
        json_error(StatusCode::BAD_REQUEST, e)
    })?;

    // Parse priority (default to Medium if not provided)
    let priority = req
        .priority
        .as_ref()
        .map(|s| parse_priority(s.as_str()))
        .transpose()
        .map_err(|e| {
            error!("Invalid priority: {}", e);
            json_error(StatusCode::BAD_REQUEST, e)
        })?
        .unwrap_or(Priority::Medium);

    // Convert steps and acceptance criteria to JSON strings
    let steps = req
        .steps
        .map(|s| {
            serde_json::to_string(&s).map_err(|e| {
                error!("Failed to serialize steps: {}", e);
                json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to serialize steps: {}", e),
                )
            })
        })
        .transpose()?;
    let acceptance_criteria = req
        .acceptance_criteria
        .map(|ac| {
            serde_json::to_string(&ac).map_err(|e| {
                error!("Failed to serialize acceptance_criteria: {}", e);
                json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to serialize acceptance_criteria: {}", e),
                )
            })
        })
        .transpose()?;
    let affected_paths = req
        .affected_paths
        .map(|paths| {
            serde_json::to_string(&paths).map_err(|e| {
                error!("Failed to serialize affected_paths: {}", e);
                json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to serialize affected_paths: {}", e),
                )
            })
        })
        .transpose()?;

    let options = CreateProposalOptions {
        title: req.title,
        description: req.description,
        category,
        suggested_priority: priority,
        steps,
        acceptance_criteria,
        affected_paths,
        estimated_complexity: None,
        depends_on: req.depends_on,
        target_project: req.target_project,
        expected_proposal_count: req.expected_proposal_count,
    };

    // Create proposal — events and dep analysis emitted inside create_proposal_impl()
    let session_id_str = session_id.as_str().to_string();
    let (proposal, dep_errors, ready_to_finalize) =
        create_proposal_impl(&state.app_state, session_id, options)
            .await
            .map_err(|e| {
                error!(
                    "Failed to create proposal for session {}: {}",
                    session_id_str, e
                );
                let status = match &e {
                    crate::error::AppError::Validation(_) => StatusCode::BAD_REQUEST,
                    crate::error::AppError::NotFound(_) => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                json_error(status, e.to_string())
            })?;

    let mut response = ProposalResponse::from(proposal);
    response.dependency_errors = dep_errors;
    response.ready_to_finalize = ready_to_finalize;
    Ok(Json(response))
}

pub async fn finalize_proposals(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<FinalizeProposalsRequest>,
) -> Result<Json<FinalizeProposalsResponse>, JsonError> {
    // Detect external request by header
    let is_external = headers
        .get(crate::http_server::handlers::external_auth::EXTERNAL_MCP_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "1")
        .unwrap_or(false);

    let response = finalize_proposals_impl(&state.app_state, &req.session_id, is_external)
        .await
        .map_err(|e| {
            error!(
                "Failed to finalize proposals for session {}: {}",
                req.session_id, e
            );
            let status = match &e {
                crate::error::AppError::Validation(_) => StatusCode::BAD_REQUEST,
                crate::error::AppError::NotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            json_error(status, e.to_string())
        })?;

    // Layer 2+3: persist and push ideation:proposals_ready (non-fatal, enriched)
    {
        let mut payload = serde_json::json!({
            "session_id": req.session_id,
            "project_id": response.project_id,
            "proposal_count": response.tasks_created,
        });
        let ctx = WebhookPresentationContext {
            project_name: response.project_name.clone(),
            session_title: response.session_title.clone(),
            task_title: None,
            presentation_kind: Some(PresentationKind::ProposalsReady),
        };
        ctx.inject_into(&mut payload);
        if let Some(ref publisher) = state.app_state.webhook_publisher {
            if let Err(e) = emit_external_webhook_event(
                "ideation:proposals_ready",
                &response.project_id,
                payload,
                &state.app_state.external_events_repo,
                publisher,
            )
            .await
            {
                tracing::warn!(
                    error = e,
                    context = "finalize_proposals: ideation:proposals_ready emit",
                    "Non-fatal error in enrichment path"
                );
            }
        } else if let Err(e) = state
            .app_state
            .external_events_repo
            .insert_event(
                "ideation:proposals_ready",
                &response.project_id,
                &payload.to_string(),
            )
            .await
        {
            tracing::warn!(error = %e, "Failed to persist ideation:proposals_ready event");
        }
    }

    if response.session_status == "accepted" {
        // Layer 2+3: persist and push ideation:session_accepted (non-fatal, enriched)
        let mut accepted_payload = serde_json::json!({
            "session_id": req.session_id,
            "project_id": response.project_id,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let accepted_ctx = WebhookPresentationContext {
            project_name: response.project_name.clone(),
            session_title: response.session_title.clone(),
            task_title: None,
            presentation_kind: Some(PresentationKind::SessionAccepted),
        };
        accepted_ctx.inject_into(&mut accepted_payload);
        if let Some(ref publisher) = state.app_state.webhook_publisher {
            if let Err(e) = emit_external_webhook_event(
                "ideation:session_accepted",
                &response.project_id,
                accepted_payload,
                &state.app_state.external_events_repo,
                publisher,
            )
            .await
            {
                tracing::warn!(
                    error = e,
                    context = "finalize_proposals: ideation:session_accepted emit",
                    "Non-fatal error in enrichment path"
                );
            }
        } else if let Err(e) = state
            .app_state
            .external_events_repo
            .insert_event(
                "ideation:session_accepted",
                &response.project_id,
                &accepted_payload.to_string(),
            )
            .await
        {
            tracing::warn!(error = %e, "Failed to persist ideation:session_accepted event");
        }

        crate::http_server::emit_http_event(
            &state,
            "ideation:session_accepted",
            serde_json::json!({
                "sessionId": req.session_id,
                "projectId": response.project_id,
            }),
        );

        // Notify task scheduler: newly created tasks may be Ready
        let project_id =
            crate::domain::entities::ProjectId::from_string(response.project_id.clone());
        let queued_count = match state
            .app_state
            .task_repo
            .get_by_status(&project_id, crate::domain::entities::InternalStatus::Ready)
            .await
        {
            Ok(tasks) => tasks.len(),
            Err(e) => {
                tracing::warn!("Failed to count Ready tasks for queue_changed event: {}", e);
                0
            }
        };
        let queued_message_count = match crate::commands::task_commands::helpers::count_slot_consuming_queued_messages_for_project(
            &state.app_state,
            &project_id,
        )
        .await
        {
            Ok(count) => count,
            Err(e) => {
                tracing::warn!(
                    "Failed to count queued agent messages for queue_changed event: {}",
                    e
                );
                0
            }
        };
        crate::http_server::emit_http_event(
            &state,
            "execution:queue_changed",
            serde_json::json!({
                "queuedCount": queued_count,
                "queuedMessageCount": queued_message_count,
                "projectId": response.project_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        );

        // Async agent cleanup — unlike apply.rs (external HTTP accept where the caller is NOT
        // the agent), here the caller IS the ideation agent via the MCP tool path. Stopping
        // synchronously would kill the MCP server stdio child before the HTTP response is
        // relayed back to the agent. We spawn a background task with a 200ms delay to ensure
        // the response traverses the multi-hop path (Axum → socket → ralphx-mcp-server →
        // stdio → agent) before pkill fires.
        let session_id_for_cleanup = req.session_id.clone();
        let app_state_for_cleanup = Arc::clone(&state.app_state);
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            let task_cleanup = TaskCleanupService::new(
                Arc::clone(&app_state_for_cleanup.task_repo),
                Arc::clone(&app_state_for_cleanup.project_repo),
                Arc::clone(&app_state_for_cleanup.running_agent_registry),
                None,
            )
            .with_interactive_process_registry(Arc::clone(
                &app_state_for_cleanup.interactive_process_registry,
            ));
            let stopped = task_cleanup
                .stop_ideation_session_agent(&session_id_for_cleanup)
                .await;
            if !stopped {
                tracing::warn!(
                    session_id = %session_id_for_cleanup,
                    "finalize_proposals cleanup: no running process found for accepted session"
                );
            }
            // Stop and archive any running verification child agents (best-effort).
            stop_verification_children(&session_id_for_cleanup, &app_state_for_cleanup)
                .await
                .ok();
        });
    }

    // Trigger scheduler to pick up newly Ready tasks (ready_settle_ms delay)
    // This is necessary because tasks are set via direct repo update, bypassing TransitionHandler
    spawn_ready_task_scheduler_if_needed(
        &state.app_state,
        Arc::clone(&state.execution_state),
        state.app_state.app_handle.as_ref().cloned(),
        response.any_ready_tasks,
    );

    Ok(Json(response))
}

pub async fn update_task_proposal(
    State(state): State<HttpServerState>,
    Json(req): Json<UpdateProposalRequest>,
) -> Result<Json<ProposalResponse>, JsonError> {
    let proposal_id = TaskProposalId::from_string(req.proposal_id);

    // Parse category if provided
    let category = req
        .category
        .as_ref()
        .map(|s| parse_category(s.as_str()))
        .transpose()
        .map_err(|e| {
            error!("Invalid category: {}", e);
            json_error(StatusCode::BAD_REQUEST, e)
        })?;

    // Parse priority if provided
    let user_priority = req
        .user_priority
        .as_ref()
        .map(|s| parse_priority(s.as_str()))
        .transpose()
        .map_err(|e| {
            error!("Invalid priority: {}", e);
            json_error(StatusCode::BAD_REQUEST, e)
        })?;

    // Convert steps and acceptance criteria to JSON strings
    let steps = req
        .steps
        .map(|s| {
            serde_json::to_string(&s).map_err(|e| {
                error!("Failed to serialize steps: {}", e);
                json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to serialize steps: {}", e),
                )
            })
        })
        .transpose()?;
    let acceptance_criteria = req
        .acceptance_criteria
        .map(|ac| {
            serde_json::to_string(&ac).map_err(|e| {
                error!("Failed to serialize acceptance_criteria: {}", e);
                json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to serialize acceptance_criteria: {}", e),
                )
            })
        })
        .transpose()?;
    let affected_paths = req
        .affected_paths
        .map(|paths| {
            serde_json::to_string(&paths).map_err(|e| {
                error!("Failed to serialize affected_paths: {}", e);
                json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to serialize affected_paths: {}", e),
                )
            })
        })
        .transpose()?;

    let options = UpdateProposalOptions {
        title: req.title,
        description: req.description.map(Some),
        category,
        steps: steps.map(Some),
        acceptance_criteria: acceptance_criteria.map(Some),
        affected_paths: affected_paths.map(Some),
        user_priority,
        estimated_complexity: None,
        source: UpdateSource::Api,
        add_depends_on: req.add_depends_on,
        add_blocks: req.add_blocks,
        target_project: req.target_project.map(Some),
    };

    // Update proposal — events and dep analysis emitted inside update_proposal_impl()
    let (updated, dep_errors) = update_proposal_impl(&state.app_state, &proposal_id, options)
        .await
        .map_err(|e| {
            error!("Failed to update proposal {}: {}", proposal_id.as_str(), e);
            let status = match &e {
                crate::error::AppError::Validation(_) => StatusCode::BAD_REQUEST,
                crate::error::AppError::NotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            json_error(status, e.to_string())
        })?;

    let mut response = ProposalResponse::from(updated);
    response.dependency_errors = dep_errors;
    Ok(Json(response))
}

pub async fn archive_task_proposal(
    State(state): State<HttpServerState>,
    Json(req): Json<DeleteProposalRequest>,
) -> Result<Json<SuccessResponse>, StatusCode> {
    let proposal_id = TaskProposalId::from_string(req.proposal_id.clone());

    // Archive proposal — assert_session_mutable(), events, and dep analysis inside impl
    archive_proposal_impl(&state.app_state, proposal_id)
        .await
        .map_err(|e| {
            error!("Failed to archive proposal {}: {}", req.proposal_id, e);
            match e {
                crate::error::AppError::NotFound(_) => StatusCode::NOT_FOUND,
                crate::error::AppError::Validation(_) => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            }
        })?;

    Ok(Json(SuccessResponse {
        success: true,
        message: "Proposal archived successfully".to_string(),
    }))
}

pub async fn update_session_title(
    State(state): State<HttpServerState>,
    Json(req): Json<UpdateSessionTitleRequest>,
) -> Result<Json<SuccessResponse>, StatusCode> {
    let title = req.title.clone();

    match (req.session_id.as_deref(), req.conversation_id.as_deref()) {
        (Some(session_id), None) => {
            let session_id = IdeationSessionId::from_string(session_id.to_string());
            state
                .app_state
                .ideation_session_repo
                .update_title(&session_id, Some(title.clone()), "auto")
                .await
                .map_err(|e| {
                    error!("Failed to update session title: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            crate::http_server::emit_http_event(
                &state,
                "ideation:session_title_updated",
                serde_json::json!({
                    "sessionId": session_id.as_str(),
                    "title": title,
                }),
            );

            Ok(Json(SuccessResponse {
                success: true,
                message: "Session title updated".to_string(),
            }))
        }
        (None, Some(conversation_id)) => {
            let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
            let updated_title = title.clone();
            state
                .app_state
                .chat_conversation_repo
                .update_title(&conversation_id, &updated_title)
                .await
                .map_err(|e| {
                    error!("Failed to update conversation title: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            let conversation = state
                .app_state
                .chat_conversation_repo
                .get_by_id(&conversation_id)
                .await
                .map_err(|e| {
                    error!("Failed to reload updated conversation title: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
                .ok_or_else(|| {
                    error!(
                        "Conversation not found after title update: {}",
                        conversation_id
                    );
                    StatusCode::NOT_FOUND
                })?;
            let synced_planning_session_id = sync_linked_planning_session_title_from_conversation(
                &state.app_state,
                &conversation_id,
                &updated_title,
            )
            .await
            .map_err(|e| {
                error!("Failed to sync linked planning session title: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            if let Some(session_id) = synced_planning_session_id {
                crate::http_server::emit_http_event(
                    &state,
                    "ideation:session_title_updated",
                    serde_json::json!({
                        "sessionId": session_id.as_str(),
                        "title": updated_title,
                    }),
                );
            }
            crate::http_server::emit_http_event(
                &state,
                "agent:conversation_title_updated",
                serde_json::json!({
                    "conversationId": conversation.id.as_str(),
                    "contextType": conversation.context_type.to_string(),
                    "contextId": conversation.context_id,
                    "title": updated_title,
                }),
            );

            Ok(Json(SuccessResponse {
                success: true,
                message: "Conversation title updated".to_string(),
            }))
        }
        (Some(_), Some(_)) | (None, None) => Err(StatusCode::BAD_REQUEST),
    }
}

pub async fn list_session_proposals(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<ListProposalsResponse>, StatusCode> {
    let session_id = IdeationSessionId::from_string(session_id);

    // Get all proposals for session
    let proposals = state
        .app_state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| {
            error!("Failed to list proposals: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get all dependencies for the session
    let all_deps = state
        .app_state
        .proposal_dependency_repo
        .get_all_for_session(&session_id)
        .await
        .map_err(|e| {
            error!("Failed to get dependencies: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Build dependency map: proposal_id -> [depends_on_ids]
    let mut dep_map: HashMap<String, Vec<String>> = HashMap::new();
    for (from, to, _reason) in all_deps {
        dep_map
            .entry(from.to_string())
            .or_default()
            .push(to.to_string());
    }

    let count = proposals.len();
    let summaries: Vec<ProposalSummary> = proposals
        .into_iter()
        .map(|p| {
            let id_str = p.id.to_string();
            let priority = p.effective_priority().to_string();
            let category = p.category.to_string();
            let plan_artifact_id = p.plan_artifact_id.map(|id| id.to_string());
            ProposalSummary {
                id: id_str.clone(),
                title: p.title,
                category,
                priority,
                depends_on: dep_map.remove(&id_str).unwrap_or_default(),
                plan_artifact_id,
                target_project: p.target_project.clone(),
            }
        })
        .collect();

    Ok(Json(ListProposalsResponse {
        proposals: summaries,
        count,
    }))
}

pub async fn get_proposal(
    State(state): State<HttpServerState>,
    Path(proposal_id): Path<String>,
) -> Result<Json<ProposalDetailResponse>, StatusCode> {
    let proposal_id = TaskProposalId::from_string(proposal_id);

    let proposal = state
        .app_state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .map_err(|e| {
            error!("Failed to get proposal: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Get dependencies for this proposal
    let deps = state
        .app_state
        .proposal_dependency_repo
        .get_dependencies(&proposal_id)
        .await
        .map_err(|e| {
            error!("Failed to get dependencies: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Parse steps and acceptance_criteria from JSON strings
    let steps: Vec<String> = proposal
        .steps
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let acceptance_criteria: Vec<String> = proposal
        .acceptance_criteria
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    // Compute values before moving fields
    let priority = proposal.effective_priority().to_string();
    let category = proposal.category.to_string();
    let created_at = proposal.created_at.to_rfc3339();
    let plan_artifact_id = proposal.plan_artifact_id.map(|id| id.to_string());

    Ok(Json(ProposalDetailResponse {
        id: proposal.id.to_string(),
        session_id: proposal.session_id.to_string(),
        title: proposal.title,
        description: proposal.description,
        category,
        priority,
        steps,
        acceptance_criteria,
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        plan_artifact_id,
        created_at,
        target_project: proposal.target_project.clone(),
    }))
}
