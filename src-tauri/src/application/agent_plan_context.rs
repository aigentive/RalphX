use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use crate::application::chat_service::MAX_ARTIFACT_REFERENCES;
use crate::application::AppState;
use crate::domain::entities::ideation::PLAN_CONTRACT_V2;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, Artifact, ArtifactId, ArtifactType, ChatConversationId,
    IdeationSession, IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus, SessionPurpose,
};
use crate::domain::repositories::PlanApprovalActor;
use crate::domain::services::ComposerArtifactReference;

#[derive(Debug, Clone)]
pub(crate) struct PlanApprovalLookup {
    pub(crate) approved_at: Option<String>,
    pub(crate) approved_by: String,
}

pub(crate) async fn lookup_plan_approval(
    state: &AppState,
    session_id: &IdeationSessionId,
    artifact: &Artifact,
    blueprint: Option<&Artifact>,
) -> Result<Option<PlanApprovalLookup>, String> {
    let approval = state
        .plan_approval_repo
        .get_by_session(session_id)
        .await
        .map_err(|error| error.to_string())?;

    Ok(approval
        .filter(|approval| approval.matches_artifacts(artifact, blueprint))
        .map(|approval| PlanApprovalLookup {
            approved_at: Some(approval.approved_at),
            approved_by: approval.approved_by,
        }))
}

pub(crate) fn plan_reference_status(
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

pub(crate) fn plan_bundle_composer_references(
    session_id: &IdeationSessionId,
    overview: &Artifact,
    blueprint: Option<&Artifact>,
    status: &str,
) -> Vec<ComposerArtifactReference> {
    let mut references = vec![plan_composer_reference(
        overview,
        session_id,
        "plan",
        "Plan Overview",
        status,
    )];
    if let Some(blueprint) = blueprint {
        references.push(plan_composer_reference(
            blueprint,
            session_id,
            "plan_blueprint",
            "Implementation Blueprint",
            status,
        ));
    }
    references
}

#[derive(Debug, Clone)]
pub(crate) struct LinkedWorkspacePlanSnapshot {
    pub(crate) session: IdeationSession,
    pub(crate) overview: Artifact,
    pub(crate) blueprint: Option<Artifact>,
    pub(crate) status: String,
    pub(crate) has_exact_approval: bool,
}

impl LinkedWorkspacePlanSnapshot {
    pub(crate) fn composer_references(&self) -> Vec<ComposerArtifactReference> {
        plan_bundle_composer_references(
            &self.session.id,
            &self.overview,
            self.blueprint.as_ref(),
            self.status.as_str(),
        )
    }

    pub(crate) fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"ralphx-linked-workspace-plan-v1\n");
        hasher.update(self.session.id.as_str().as_bytes());
        hasher.update(b"\noverview\n");
        hasher.update(self.overview.id.as_str().as_bytes());
        hasher.update(b"\n");
        hasher.update(self.overview.metadata.version.to_string().as_bytes());
        if let Some(blueprint) = self.blueprint.as_ref() {
            hasher.update(b"\nblueprint\n");
            hasher.update(blueprint.id.as_str().as_bytes());
            hasher.update(b"\n");
            hasher.update(blueprint.metadata.version.to_string().as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}

pub(crate) async fn admit_linked_edit_plan_references(
    state: &AppState,
    conversation_id: &ChatConversationId,
    references: Vec<ComposerArtifactReference>,
    require_approved_plan: bool,
) -> Result<Vec<ComposerArtifactReference>, String> {
    let Some(context) = load_linked_edit_plan_context(state, conversation_id).await? else {
        if require_approved_plan {
            return Err(
                "Direct implementation requires an active linked plan bundle in Edit mode."
                    .to_string(),
            );
        }
        return Ok(references);
    };

    if require_approved_plan && !context.has_exact_approval {
        return Err(
            "The plan changed after direct implementation activation. Return to Plan mode, review the current bundle, and approve it again."
                .to_string(),
        );
    }
    let authoritative = context.composer_references();
    Ok(merge_authoritative_plan_references(
        authoritative,
        references,
    ))
}

async fn load_linked_edit_plan_context(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<Option<LinkedWorkspacePlanSnapshot>, String> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Ok(None);
    }
    load_linked_workspace_plan_snapshot(state, &workspace).await
}

pub(crate) async fn load_linked_workspace_plan_snapshot(
    state: &AppState,
    workspace: &crate::domain::entities::AgentConversationWorkspace,
) -> Result<Option<LinkedWorkspacePlanSnapshot>, String> {
    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return Ok(None);
    };
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "The linked planning session no longer exists. Return to Plan mode and refresh the plan."
                .to_string()
        })?;
    if session.project_id != workspace.project_id {
        return Err(
            "The linked planning session belongs to a different project. Return to Plan mode and relink the plan."
                .to_string(),
        );
    }
    if session.session_flow != IdeationSessionFlow::Planning
        || session.session_purpose == SessionPurpose::Verification
        || session.status == IdeationSessionStatus::Archived
        || session.archived_at.is_some()
    {
        return Err(
            "The linked planning session is not an active Agent plan. Return to Plan mode and refresh the plan."
                .to_string(),
        );
    }
    validate_session_conversation_ownership(state, &session, &workspace.conversation_id).await?;

    let Some(overview_id) = session
        .plan_artifact_id
        .as_ref()
        .or(session.inherited_plan_artifact_id.as_ref())
    else {
        return Ok(None);
    };
    let blueprint_id = if session.plan_artifact_id.is_some() {
        session.plan_blueprint_artifact_id.as_ref()
    } else {
        session.inherited_plan_blueprint_artifact_id.as_ref()
    };
    if session.plan_contract_version >= PLAN_CONTRACT_V2 && blueprint_id.is_none() {
        return Err(
            "The linked plan is missing its implementation blueprint. Return to Plan mode and complete the Overview and Blueprint bundle before continuing."
                .to_string(),
        );
    }

    let overview = load_current_plan_artifact(state, overview_id, "overview").await?;
    let blueprint = match blueprint_id {
        Some(blueprint_id) => {
            Some(load_current_plan_artifact(state, blueprint_id, "implementation blueprint").await?)
        }
        None => None,
    };
    let approval = lookup_plan_approval(state, &session.id, &overview, blueprint.as_ref()).await?;
    let status = plan_reference_status(&session, approval.as_ref());

    Ok(Some(LinkedWorkspacePlanSnapshot {
        session,
        overview,
        blueprint,
        status,
        has_exact_approval: approval
            .as_ref()
            .is_some_and(|approval| approval.approved_by == PlanApprovalActor::User.as_str()),
    }))
}

async fn validate_session_conversation_ownership(
    state: &AppState,
    session: &IdeationSession,
    conversation_id: &ChatConversationId,
) -> Result<(), String> {
    match (
        session.source_context_type.as_deref(),
        session.source_context_id.as_deref(),
    ) {
        (Some("agent_conversation"), Some(source_conversation_id))
            if source_conversation_id == conversation_id.as_str() =>
        {
            Ok(())
        }
        (None, None) => {
            let owner = state
                .agent_conversation_workspace_repo
                .get_by_linked_ideation_session_id(&session.id)
                .await
                .map_err(|error| error.to_string())?;
            if owner
                .as_ref()
                .is_some_and(|owner| owner.conversation_id == *conversation_id)
            {
                Ok(())
            } else {
                Err(
                    "The linked planning session belongs to a different Agent conversation. Return to Plan mode and relink the plan."
                        .to_string(),
                )
            }
        }
        _ => Err(
            "The linked planning session belongs to a different Agent conversation. Return to Plan mode and relink the plan."
                .to_string(),
        ),
    }
}

async fn load_current_plan_artifact(
    state: &AppState,
    artifact_id: &ArtifactId,
    member_name: &str,
) -> Result<Artifact, String> {
    let latest_id = state
        .artifact_repo
        .resolve_latest_artifact_id(artifact_id)
        .await
        .map_err(|error| error.to_string())?;
    let artifact = state
        .artifact_repo
        .get_by_id(&latest_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "The linked plan {member_name} no longer exists. Return to Plan mode and refresh the plan."
            )
        })?;
    if artifact.artifact_type != ArtifactType::Specification || artifact.archived_at.is_some() {
        return Err(format!(
            "The linked plan {member_name} is not an active specification. Return to Plan mode and refresh the plan."
        ));
    }
    Ok(artifact)
}

pub(crate) fn merge_authoritative_plan_references(
    authoritative: Vec<ComposerArtifactReference>,
    references: Vec<ComposerArtifactReference>,
) -> Vec<ComposerArtifactReference> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::with_capacity(MAX_ARTIFACT_REFERENCES);
    for reference in authoritative.into_iter().chain(
        references
            .into_iter()
            .filter(|reference| !is_plan_reference(reference)),
    ) {
        if seen.insert(reference.artifact_id.clone()) {
            merged.push(reference);
        }
        if merged.len() == MAX_ARTIFACT_REFERENCES {
            break;
        }
    }
    merged
}

fn plan_composer_reference(
    artifact: &Artifact,
    session_id: &IdeationSessionId,
    kind: &str,
    fallback_title: &str,
    status: &str,
) -> ComposerArtifactReference {
    ComposerArtifactReference {
        artifact_id: artifact.id.as_str().to_string(),
        kind: kind.to_string(),
        title: Some(if artifact.name.trim().is_empty() {
            fallback_title.to_string()
        } else {
            artifact.name.clone()
        }),
        session_id: Some(session_id.as_str().to_string()),
        version: Some(artifact.metadata.version),
        status: Some(status.to_string()),
    }
}

fn is_plan_reference(reference: &ComposerArtifactReference) -> bool {
    matches!(reference.kind.trim(), "plan" | "plan_blueprint")
}
