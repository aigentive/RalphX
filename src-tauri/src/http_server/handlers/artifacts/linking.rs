use super::*;

pub async fn link_proposals_to_plan(
    State(state): State<HttpServerState>,
    Json(req): Json<LinkProposalsToPlanRequest>,
) -> Result<Json<SuccessResponse>, HttpError> {
    let input_artifact_id = req.artifact_id.clone();
    let proposal_id_strs = req.proposal_ids;

    state
        .app_state
        .db
        .run_transaction(move |conn| {
            let artifact_id_str = ArtifactRepo::resolve_latest_sync(conn, &input_artifact_id)?;

            let artifact =
                ArtifactRepo::get_by_id_sync(conn, &artifact_id_str)?.ok_or_else(|| {
                    AppError::NotFound(format!("Artifact {} not found", artifact_id_str))
                })?;

            let owning_sessions =
                SessionRepo::get_by_plan_artifact_id_sync(conn, &artifact_id_str)?;
            let session = match owning_sessions.as_slice() {
                [session] => session,
                [] => {
                    return Err(AppError::Validation(format!(
                        "Artifact {} is not the current Overview for an ideation session",
                        artifact_id_str
                    )))
                }
                _ => {
                    return Err(AppError::Validation(format!(
                        "Artifact {} belongs to multiple ideation sessions",
                        artifact_id_str
                    )))
                }
            };
            crate::http_server::helpers::assert_session_mutable(session)?;

            let bundle = session.plan_artifact_bundle().ok_or_else(|| {
                AppError::Validation(
                    "Proposals require a complete plan Overview and Blueprint bundle".to_string(),
                )
            })?;
            let blueprint = match bundle.blueprint_id {
                Some(blueprint_id) => Some(
                    ArtifactRepo::get_by_id_sync(conn, blueprint_id.as_str())?.ok_or_else(
                        || {
                            AppError::NotFound(format!(
                                "Plan Blueprint artifact {} not found",
                                blueprint_id
                            ))
                        },
                    )?,
                ),
                None => None,
            };

            ProposalRepo::batch_link_proposals_sync(
                conn,
                &proposal_id_strs,
                &artifact_id_str,
                artifact.metadata.version,
                blueprint.as_ref().map(|artifact| artifact.id.as_str()),
                blueprint.as_ref().map(|artifact| artifact.metadata.version),
            )?;

            Ok(())
        })
        .await
        .map_err(|e| {
            error!("link_proposals_to_plan transaction failed: {}", e);
            map_app_err(e)
        })?;

    Ok(Json(SuccessResponse {
        success: true,
        message: "Proposals linked to plan successfully".to_string(),
    }))
}
