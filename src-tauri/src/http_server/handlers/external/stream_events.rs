use super::*;

/// GET /api/external/events/stream
/// Server-Sent Events endpoint for real-time task state change notifications.
///
/// Accepts an optional `project_id` query parameter to filter events.
/// Polls the external_events table every 2 seconds, emitting new events as SSE.
pub async fn stream_events_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Query(params): Query<StreamEventsQuery>,
) -> Result<
    axum::response::sse::Sse<
        impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    StatusCode,
> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::stream;
    use futures::StreamExt as _;

    let project_id_filter = params.project_id.clone();

    // Validate project scope if project_id provided
    if let Some(ref pid) = project_id_filter {
        let project_id = ProjectId::from_string(pid.clone());
        let project = state
            .app_state
            .project_repo
            .get_by_id(&project_id)
            .await
            .map_err(|e| {
                error!("Failed to get project {}: {}", pid, e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .ok_or(StatusCode::NOT_FOUND)?;
        project.assert_project_scope(&scope).map_err(|e| e.status)?;
    }

    let db = state.app_state.db.clone();

    // Start from the most-recent existing event (only push NEW events from this point on)
    let initial_cursor: i64 = {
        let pid_clone = project_id_filter.clone();
        db.run(move |conn| {
            let row: i64 = if let Some(ref pid) = pid_clone {
                conn.query_row(
                    "SELECT COALESCE(MAX(id), 0) FROM external_events WHERE project_id = ?1",
                    rusqlite::params![pid],
                    |r| r.get(0),
                )
                .unwrap_or(0)
            } else {
                conn.query_row(
                    "SELECT COALESCE(MAX(id), 0) FROM external_events",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0)
            };
            Ok(row)
        })
        .await
        .unwrap_or(0)
    };

    // Build SSE stream via unfold — polls every 2 seconds
    let sse_stream = stream::unfold(
        (db, project_id_filter, scope, initial_cursor),
        |(db, project_id_filter, scope, cursor)| async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            let pid_clone = project_id_filter.clone();
            let rows = db
                .run(move |conn| {
                    if let Some(ref pid) = pid_clone {
                        let mut stmt = conn
                            .prepare(
                                "SELECT id, event_type, project_id, payload, created_at \
                                 FROM external_events WHERE id > ?1 AND project_id = ?2 \
                                 ORDER BY id ASC LIMIT 50",
                            )
                            .map_err(|e| crate::error::AppError::Database(e.to_string()))?;
                        let mut result = Vec::new();
                        let rows = stmt
                            .query_map(rusqlite::params![cursor, pid], |row| {
                                Ok((
                                    row.get::<_, i64>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, String>(2)?,
                                    row.get::<_, String>(3)?,
                                    row.get::<_, String>(4)?,
                                ))
                            })
                            .map_err(|e| crate::error::AppError::Database(e.to_string()))?;
                        for row in rows {
                            result.push(
                                row.map_err(|e| crate::error::AppError::Database(e.to_string()))?,
                            );
                        }
                        Ok(result)
                    } else {
                        let mut stmt = conn
                            .prepare(
                                "SELECT id, event_type, project_id, payload, created_at \
                                 FROM external_events WHERE id > ?1 \
                                 ORDER BY id ASC LIMIT 50",
                            )
                            .map_err(|e| crate::error::AppError::Database(e.to_string()))?;
                        let mut result = Vec::new();
                        let rows = stmt
                            .query_map(rusqlite::params![cursor], |row| {
                                Ok((
                                    row.get::<_, i64>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, String>(2)?,
                                    row.get::<_, String>(3)?,
                                    row.get::<_, String>(4)?,
                                ))
                            })
                            .map_err(|e| crate::error::AppError::Database(e.to_string()))?;
                        for row in rows {
                            result.push(
                                row.map_err(|e| crate::error::AppError::Database(e.to_string()))?,
                            );
                        }
                        Ok(result)
                    }
                })
                .await
                .unwrap_or_default();

            // Enforce scope allowlist
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|(_, _, proj_id, _, _)| {
                    if let Some(ref allowed) = scope.0 {
                        allowed.iter().any(|p| p.to_string() == *proj_id)
                    } else {
                        true
                    }
                })
                .collect();

            let new_cursor = rows.last().map(|(id, _, _, _, _)| *id).unwrap_or(cursor);

            let events: Vec<Result<Event, std::convert::Infallible>> = rows
                .into_iter()
                .map(|(id, event_type, proj_id, payload, created_at)| {
                    let data = serde_json::json!({
                        "id": id,
                        "event_type": event_type,
                        "project_id": proj_id,
                        "payload": serde_json::from_str::<serde_json::Value>(&payload)
                            .unwrap_or(serde_json::json!({})),
                        "created_at": created_at,
                    });
                    Ok(Event::default().event(event_type).data(data.to_string()))
                })
                .collect();

            Some((
                stream::iter(events),
                (db, project_id_filter, scope, new_cursor),
            ))
        },
    )
    .flat_map(|s| s);

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()))
}

#[derive(Debug, Deserialize)]
pub struct StreamEventsQuery {
    pub project_id: Option<String>,
}
