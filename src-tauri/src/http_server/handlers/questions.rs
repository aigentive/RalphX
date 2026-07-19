use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use super::*;
use crate::application::harness_runtime_registry::default_external_mcp_human_wait_timeout_secs;
use crate::application::interactive_notification_producer::{
    question_notification_key, InteractiveNotificationProducer,
};
use crate::application::{NotificationContextResolver, QuestionAnswer, QuestionOption};
use crate::domain::entities::ChatConversationId;
use crate::domain::entities::NotificationTargetKind;

pub async fn request_question(
    State(state): State<HttpServerState>,
    Json(input): Json<QuestionRequestInput>,
) -> Json<QuestionRequestResponse> {
    let request_id = input
        .request_id
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Convert input options to domain QuestionOption
    let options: Vec<QuestionOption> = input
        .options
        .iter()
        .map(|o| QuestionOption {
            value: o.value.clone(),
            label: o.label.clone(),
            description: o.description.clone(),
        })
        .collect();

    // Register in QuestionState
    state
        .app_state
        .question_state
        .register_with_metadata(
            request_id.clone(),
            input.session_id.clone(),
            input.question.clone(),
            input.header.clone(),
            options,
            input.multi_select,
            input.allow_skip,
            input.batch_index,
            input.batch_total,
            input.metadata.clone(),
        )
        .await;

    let notification_context = NotificationContextResolver::from_app_state(&state.app_state);
    match notification_context
        .resolve_conversation_target(&ChatConversationId::from_string(input.session_id.clone()))
        .await
    {
        Ok(resolved) if resolved.target.kind != NotificationTargetKind::None => {
            state
                .app_state
                .notification_service()
                .record(InteractiveNotificationProducer::agent_question(
                    &request_id,
                    &input.question,
                    resolved,
                ))
                .await;
        }
        Ok(_) => {
            tracing::warn!(request_id = %request_id, "Skipped question notification without a navigable target")
        }
        Err(error) => {
            tracing::warn!(error = %error, request_id = %request_id, "Failed to resolve question notification context");
        }
    }

    crate::http_server::emit_http_event(
        &state,
        "agent:ask_user_question",
        serde_json::json!({
            "requestId": &request_id,
            "sessionId": &input.session_id,
            "question": &input.question,
            "header": &input.header,
            "options": &input.options,
            "multiSelect": input.multi_select,
            "allowSkip": input.allow_skip,
            "batchIndex": input.batch_index,
            "batchTotal": input.batch_total,
            "metadata": &input.metadata,
        }),
    );

    Json(QuestionRequestResponse { request_id })
}

fn question_wait_timeout() -> tokio::time::Duration {
    tokio::time::Duration::from_secs(default_external_mcp_human_wait_timeout_secs())
}

async fn resolved_answer_or_timeout(
    state: &HttpServerState,
    request_id: &str,
) -> Result<Json<QuestionAnswer>, StatusCode> {
    match state
        .app_state
        .question_state
        .get_resolved_answer(request_id)
        .await
    {
        Ok(Some(answer)) => Ok(Json(answer)),
        _ => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

async fn expire_question_wait(
    state: &HttpServerState,
    request_id: &str,
) -> Result<Json<QuestionAnswer>, StatusCode> {
    if state
        .app_state
        .question_state
        .expire(request_id)
        .await
        .is_some()
    {
        state
            .app_state
            .notification_service()
            .resolve_workflow_notification(&question_notification_key(request_id))
            .await;
        Err(StatusCode::REQUEST_TIMEOUT)
    } else {
        resolved_answer_or_timeout(state, request_id).await
    }
}

pub async fn await_question(
    State(state): State<HttpServerState>,
    Path(request_id): Path<String>,
) -> Result<Json<QuestionAnswer>, StatusCode> {
    // Three-way branch:
    // (1) Found in HashMap → subscribe + wait for answer
    // (2) Not in HashMap, but resolved answer in DB → return it directly (race window)
    // (3) Not in HashMap, no resolved answer → NOT_FOUND (unknown request_id)
    let maybe_rx = {
        let pending = state.app_state.question_state.pending.lock().await;
        pending.get(&request_id).map(|req| req.sender.subscribe())
    };

    let mut rx = match maybe_rx {
        Some(rx) => rx,
        None => {
            // HashMap miss — check if already resolved (race: resolve() ran before we got here)
            match state
                .app_state
                .question_state
                .get_resolved_answer(&request_id)
                .await
            {
                Ok(Some(answer)) => return Ok(Json(answer)),
                Ok(None) => return Err(StatusCode::NOT_FOUND),
                Err(_) => return Err(StatusCode::NOT_FOUND),
            }
        }
    };

    // Keep the backend deadline ahead of the effective MCP tool ceiling so this
    // path can expire the question cleanly and return a structured 408.
    let timeout = question_wait_timeout();
    let start = tokio::time::Instant::now();

    loop {
        // Check if value is Some
        let maybe_answer: Option<QuestionAnswer> = {
            let current = rx.borrow();
            current.clone()
        };

        if let Some(answer) = maybe_answer {
            // resolve() already removed from HashMap; remove() is idempotent (no-op if gone)
            state.app_state.question_state.remove(&request_id).await;
            return Ok(Json(answer));
        }

        // Check timeout
        if start.elapsed() >= timeout {
            return expire_question_wait(&state, &request_id).await;
        }

        // Wait for change with remaining timeout
        let remaining = timeout.saturating_sub(start.elapsed());
        match tokio::time::timeout(remaining, rx.changed()).await {
            Ok(Ok(())) => continue,
            Ok(Err(_)) => {
                // Sender dropped — resolve() ran concurrently and removed the entry.
                // Fall back to DB; if there is no resolved answer, the question
                // was expired or otherwise closed without an answer.
                return resolved_answer_or_timeout(&state, &request_id).await;
            }
            Err(_) => {
                return expire_question_wait(&state, &request_id).await;
            }
        }
    }
}

pub async fn resolve_question(
    State(state): State<HttpServerState>,
    Json(input): Json<ResolveQuestionInput>,
) -> StatusCode {
    let result = state
        .app_state
        .question_state
        .resolve(
            &input.request_id,
            QuestionAnswer {
                selected_options: input.selected_options,
                text: input.text,
                skipped: input.skipped,
            },
        )
        .await;

    if result.resolved {
        state
            .app_state
            .notification_service()
            .resolve_workflow_notification(&question_notification_key(&input.request_id))
            .await;
        if let Some(ref sid) = result.session_id {
            crate::http_server::emit_http_event(
                &state,
                "agent:question_resolved",
                serde_json::json!({
                    "sessionId": sid,
                    "requestId": &input.request_id,
                }),
            );
        }
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

#[cfg(test)]
#[path = "questions_tests.rs"]
mod tests;
