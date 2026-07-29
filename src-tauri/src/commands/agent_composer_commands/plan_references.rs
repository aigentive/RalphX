use tauri::State;

use super::types::{
    AgentComposerPlanReferenceResponse, SearchAgentComposerPlanReferencesInput,
    SearchAgentComposerPlanReferencesResponse,
};
use crate::application::agent_plan_context::{lookup_plan_approval, plan_reference_status};
use crate::application::AppState;
use crate::domain::entities::{
    IdeationSession, IdeationSessionFlow, IdeationSessionStatus, ProjectId, SessionPurpose,
};

const DEFAULT_PLAN_REFERENCE_LIMIT: usize = 12;
const MAX_PLAN_REFERENCE_LIMIT: usize = 50;

#[tauri::command]
pub async fn search_agent_composer_plan_references(
    input: SearchAgentComposerPlanReferencesInput,
    state: State<'_, AppState>,
) -> Result<SearchAgentComposerPlanReferencesResponse, String> {
    search_agent_composer_plan_references_for_app_state(&state, input).await
}

/// The command body, against a plain `AppState` so its error paths are reachable in tests.
pub async fn search_agent_composer_plan_references_for_app_state(
    state: &AppState,
    input: SearchAgentComposerPlanReferencesInput,
) -> Result<SearchAgentComposerPlanReferencesResponse, String> {
    let project_id = ProjectId::from_string(input.project_id);
    let query = normalize_query(&input.query);
    let limit = input
        .limit
        .unwrap_or(DEFAULT_PLAN_REFERENCE_LIMIT)
        .min(MAX_PLAN_REFERENCE_LIMIT);

    let sessions = state
        .ideation_session_repo
        .get_by_project(&project_id)
        .await
        .map_err(|error| error.to_string())?;

    let mut candidates = Vec::<ScoredPlanReference>::new();
    for session in sessions {
        if !session_can_reference_plan(&session) {
            continue;
        }

        let Some(seed_artifact_id) = session
            .plan_artifact_id
            .as_ref()
            .or(session.inherited_plan_artifact_id.as_ref())
        else {
            continue;
        };

        // NOT `unwrap_or_else(seed_id)`: falling back to the pre-resolution id made the
        // following `get_by_id` miss and `continue`, so a resolver outage dropped sessions from
        // the list silently — and `truncated` is computed from `limit` alone, so the short list
        // shipped looking complete. The sibling resolver call below already propagates.
        let latest_artifact_id = state
            .artifact_repo
            .resolve_latest_artifact_id(seed_artifact_id)
            .await
            .map_err(|error| error.to_string())?;
        let Some(artifact) = state
            .artifact_repo
            .get_by_id(&latest_artifact_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            continue;
        };

        let approval = if session.session_flow == IdeationSessionFlow::Planning {
            if let Some(bundle) = session.plan_artifact_bundle() {
                let blueprint = if let Some(blueprint_id) = bundle.blueprint_id.as_ref() {
                    let latest_blueprint_id = state
                        .artifact_repo
                        .resolve_latest_artifact_id(blueprint_id)
                        .await
                        .map_err(|error| error.to_string())?;
                    state
                        .artifact_repo
                        .get_by_id(&latest_blueprint_id)
                        .await
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| {
                            format!(
                                "Current plan blueprint artifact not found: {}",
                                latest_blueprint_id.as_str()
                            )
                        })
                        .map(Some)?
                } else {
                    None
                };
                lookup_plan_approval(&state, &session.id, &artifact, blueprint.as_ref()).await?
            } else {
                None
            }
        } else {
            None
        };
        let status = plan_reference_status(&session, approval.as_ref());
        let title = session
            .title
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| Some(artifact.name.clone()));

        let response = AgentComposerPlanReferenceResponse {
            session_id: session.id.as_str().to_string(),
            artifact_id: artifact.id.as_str().to_string(),
            title,
            status,
            artifact_version: artifact.metadata.version,
            updated_at: session.updated_at.to_rfc3339(),
            approved_at: approval.and_then(|approved| approved.approved_at),
        };
        let Some(score) = score_reference(&response, &query) else {
            continue;
        };
        candidates.push(ScoredPlanReference {
            score,
            updated_at: session.updated_at,
            response,
        });
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| {
                left.response
                    .title
                    .as_deref()
                    .unwrap_or("")
                    .cmp(right.response.title.as_deref().unwrap_or(""))
            })
    });

    let total = candidates.len();
    let plans = candidates
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.response)
        .collect::<Vec<_>>();

    Ok(SearchAgentComposerPlanReferencesResponse {
        plans,
        truncated: total > limit,
    })
}

struct ScoredPlanReference {
    score: u8,
    updated_at: chrono::DateTime<chrono::Utc>,
    response: AgentComposerPlanReferenceResponse,
}

pub(crate) fn session_can_reference_plan(session: &IdeationSession) -> bool {
    session.session_purpose != SessionPurpose::Verification
        && session.status != IdeationSessionStatus::Archived
        && (session.plan_artifact_id.is_some() || session.inherited_plan_artifact_id.is_some())
}

pub(crate) fn score_reference(
    reference: &AgentComposerPlanReferenceResponse,
    query: &str,
) -> Option<u8> {
    if query.is_empty() {
        return Some(1);
    }

    let title = reference.title.as_deref().unwrap_or("").to_lowercase();
    let session_id = reference.session_id.to_lowercase();
    let artifact_id = reference.artifact_id.to_lowercase();
    let status = reference.status.to_lowercase();

    if artifact_id == query || session_id == query {
        return Some(100);
    }
    if title == query {
        return Some(90);
    }
    if title.starts_with(query) {
        return Some(70);
    }
    if title.contains(query) {
        return Some(55);
    }
    if artifact_id.contains(query) || session_id.contains(query) || status.contains(query) {
        return Some(35);
    }
    None
}

pub(crate) fn normalize_query(query: &str) -> String {
    query.trim().to_lowercase()
}
