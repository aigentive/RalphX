use rusqlite::OptionalExtension;
use tauri::State;

use super::types::{
    AgentComposerPlanReferenceResponse, SearchAgentComposerPlanReferencesInput,
    SearchAgentComposerPlanReferencesResponse,
};
use crate::application::AppState;
use crate::domain::entities::{
    ArtifactId, IdeationSession, IdeationSessionFlow, IdeationSessionStatus, ProjectId,
    SessionPurpose,
};

const DEFAULT_PLAN_REFERENCE_LIMIT: usize = 12;
const MAX_PLAN_REFERENCE_LIMIT: usize = 50;

#[tauri::command]
pub async fn search_agent_composer_plan_references(
    input: SearchAgentComposerPlanReferencesInput,
    state: State<'_, AppState>,
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

        let latest_artifact_id = state
            .artifact_repo
            .resolve_latest_artifact_id(seed_artifact_id)
            .await
            .unwrap_or_else(|_| ArtifactId::from_string(seed_artifact_id.as_str()));
        let Some(artifact) = state
            .artifact_repo
            .get_by_id(&latest_artifact_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            continue;
        };

        let approval = if session.session_flow == IdeationSessionFlow::Planning {
            lookup_plan_approval(
                &state,
                session.id.as_str(),
                artifact.id.as_str(),
                artifact.metadata.version,
            )
            .await?
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

struct PlanApprovalLookup {
    approved_artifact_id: String,
    approved_version: i64,
    approved_at: Option<String>,
}

fn session_can_reference_plan(session: &IdeationSession) -> bool {
    session.session_purpose != SessionPurpose::Verification
        && session.status != IdeationSessionStatus::Archived
        && (session.plan_artifact_id.is_some() || session.inherited_plan_artifact_id.is_some())
}

fn plan_reference_status(
    session: &IdeationSession,
    approval: Option<&PlanApprovalLookup>,
) -> String {
    if session.status == IdeationSessionStatus::Accepted {
        return "accepted".to_string();
    }
    if approval.is_some() {
        return "approved".to_string();
    }
    "draft".to_string()
}

async fn lookup_plan_approval(
    state: &AppState,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
) -> Result<Option<PlanApprovalLookup>, String> {
    let session_id = session_id.to_string();
    let artifact_id = artifact_id.to_string();
    let approval = state
        .db
        .run(move |conn| {
            let row = conn
                .query_row(
                    "SELECT artifact_id, artifact_version, approved_at
                     FROM plan_artifact_approvals
                     WHERE session_id = ?1 AND status = 'approved'",
                    [session_id.as_str()],
                    |row| {
                        Ok(PlanApprovalLookup {
                            approved_artifact_id: row.get(0)?,
                            approved_version: row.get(1)?,
                            approved_at: row.get(2)?,
                        })
                    },
                )
                .optional()?;
            Ok(row)
        })
        .await
        .map_err(|error| error.to_string())?;

    Ok(approval.filter(|row| {
        row.approved_artifact_id == artifact_id
            && row.approved_version == i64::from(artifact_version)
    }))
}

fn score_reference(reference: &AgentComposerPlanReferenceResponse, query: &str) -> Option<u8> {
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

fn normalize_query(query: &str) -> String {
    query.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn plan_reference(
        title: Option<&str>,
        session_id: &str,
        artifact_id: &str,
        status: &str,
    ) -> AgentComposerPlanReferenceResponse {
        AgentComposerPlanReferenceResponse {
            session_id: session_id.to_string(),
            artifact_id: artifact_id.to_string(),
            title: title.map(str::to_string),
            status: status.to_string(),
            artifact_version: 4,
            updated_at: "2026-06-12T00:00:00Z".to_string(),
            approved_at: None,
        }
    }

    fn referenceable_session() -> IdeationSession {
        IdeationSession::builder()
            .project_id(ProjectId::from_string("project-1".to_string()))
            .plan_artifact_id(ArtifactId::from_string("artifact-1"))
            .updated_at(Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap())
            .build()
    }

    #[test]
    fn normalize_query_trims_and_lowercases() {
        assert_eq!(normalize_query("  Plan Alpha  "), "plan alpha");
    }

    #[test]
    fn score_reference_ranks_identifier_title_and_status_matches() {
        assert_eq!(
            score_reference(
                &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
                ""
            ),
            Some(1)
        );
        assert_eq!(
            score_reference(
                &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
                "artifact-1"
            ),
            Some(100)
        );
        assert_eq!(
            score_reference(
                &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
                "plan alpha"
            ),
            Some(90)
        );
        assert_eq!(
            score_reference(
                &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
                "plan"
            ),
            Some(70)
        );
        assert_eq!(
            score_reference(
                &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
                "alpha"
            ),
            Some(55)
        );
        assert_eq!(
            score_reference(
                &plan_reference(None, "session-1", "artifact-1", "approved"),
                "approve"
            ),
            Some(35)
        );
        assert_eq!(
            score_reference(
                &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
                "missing"
            ),
            None
        );
    }

    #[test]
    fn session_can_reference_owned_or_inherited_active_non_verification_plan() {
        assert!(session_can_reference_plan(&referenceable_session()));

        let inherited_session = IdeationSession::builder()
            .project_id(ProjectId::from_string("project-1".to_string()))
            .inherited_plan_artifact_id(ArtifactId::from_string("artifact-parent"))
            .build();
        assert!(session_can_reference_plan(&inherited_session));

        let mut archived = referenceable_session();
        archived.status = IdeationSessionStatus::Archived;
        assert!(!session_can_reference_plan(&archived));

        let mut verification = referenceable_session();
        verification.session_purpose = SessionPurpose::Verification;
        assert!(!session_can_reference_plan(&verification));

        let no_plan = IdeationSession::builder()
            .project_id(ProjectId::from_string("project-1".to_string()))
            .build();
        assert!(!session_can_reference_plan(&no_plan));
    }

    #[test]
    fn plan_reference_status_prefers_accepted_then_approved_then_draft() {
        let mut accepted = referenceable_session();
        accepted.status = IdeationSessionStatus::Accepted;
        assert_eq!(plan_reference_status(&accepted, None), "accepted");

        let approval = PlanApprovalLookup {
            approved_artifact_id: "artifact-1".to_string(),
            approved_version: 4,
            approved_at: Some("2026-06-12T00:00:00Z".to_string()),
        };
        let active = referenceable_session();
        assert_eq!(plan_reference_status(&active, Some(&approval)), "approved");
        assert_eq!(plan_reference_status(&active, None), "draft");
    }
}
