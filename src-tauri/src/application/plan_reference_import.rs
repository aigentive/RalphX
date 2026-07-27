use tracing::warn;

use crate::application::agent_planning_session_titles::hydrate_agent_conversation_planning_session_title;
use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::AppState;
use crate::domain::entities::ideation::PLAN_CONTRACT_V2;
use crate::domain::entities::{
    AgentConversationWorkspace, Artifact, ArtifactId, ArtifactMetadata, ArtifactRelation,
    ArtifactType, IdeationSession, IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus,
    Project, SessionPurpose,
};
use crate::domain::services::ComposerArtifactReference;

const PLAN_REFERENCE_IMPORT_CREATOR: &str = "agent_plan_reference_import";
const PLAN_REFERENCE_IMPORT_REASON: &str = "agent_plan_reference_import";

#[derive(Debug, Clone)]
pub(crate) struct AgentPlanReferenceImport {
    pub composer_references: Vec<ComposerArtifactReference>,
}

pub(crate) fn selected_plan_reference(
    references: &[ComposerArtifactReference],
) -> Result<Option<ComposerArtifactReference>, String> {
    let plan_references = references
        .iter()
        .filter(|reference| is_plan_reference(reference))
        .collect::<Vec<_>>();

    if plan_references.len() > 1 {
        return Err(
            "Multiple plan references selected. Choose exactly one plan reference to start an agent conversation."
                .to_string(),
        );
    }

    Ok(plan_references
        .first()
        .map(|reference| (*reference).clone()))
}

pub(crate) fn rewrite_imported_plan_references(
    references: &[ComposerArtifactReference],
    source_reference: &ComposerArtifactReference,
    imported_references: &[ComposerArtifactReference],
) -> Vec<ComposerArtifactReference> {
    references
        .iter()
        .flat_map(|reference| {
            if is_same_plan_reference(reference, source_reference) {
                imported_references.to_vec()
            } else {
                vec![reference.clone()]
            }
        })
        .collect()
}

pub(crate) async fn import_agent_conversation_plan_reference(
    state: &AppState,
    project: &Project,
    workspace: &mut AgentConversationWorkspace,
    reference: &ComposerArtifactReference,
) -> Result<AgentPlanReferenceImport, String> {
    let source_artifact_id = clean_required_value(&reference.artifact_id, "Plan artifact id")?;
    let source_session_id = reference
        .session_id
        .as_deref()
        .map(|value| clean_required_value(value, "Plan reference session_id"))
        .transpose()?
        .ok_or_else(|| "Plan reference is missing session_id".to_string())?;
    let source_session_id = IdeationSessionId::from_string(source_session_id);

    let source_artifact = state
        .artifact_repo
        .get_by_id(&ArtifactId::from_string(source_artifact_id.clone()))
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Source plan artifact not found: {source_artifact_id}"))?;
    if !matches!(source_artifact.artifact_type, ArtifactType::Specification) {
        return Err("Source artifact is not a specification/plan type".to_string());
    }

    let source_session = state
        .ideation_session_repo
        .get_by_id(&source_session_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Source session not found: {}", source_session_id.as_str()))?;
    if source_session.project_id != project.id {
        return Err("Source session belongs to a different project".to_string());
    }
    if source_session.status == IdeationSessionStatus::Archived {
        return Err("Cannot import from an archived session".to_string());
    }
    if source_session.session_purpose == SessionPurpose::Verification {
        return Err("Cannot import from a verification child session".to_string());
    }
    let source_bundle = source_session
        .plan_artifact_bundle()
        .ok_or_else(|| "Source session has an incomplete plan bundle".to_string())?;
    if source_bundle.contract_version < PLAN_CONTRACT_V2 || source_bundle.blueprint_id.is_none() {
        return Err(
            "This plan predates implementation blueprints. Open Plan mode and generate a complete Overview and Blueprint bundle before importing it into Edit mode."
                .to_string(),
        );
    }
    if source_bundle.overview_id != source_artifact.id {
        return Err("Selected artifact is not the source session's current overview".to_string());
    }

    let cloned_artifact =
        clone_plan_artifact_for_import(state, &source_artifact, PLAN_REFERENCE_IMPORT_CREATOR)
            .await?;
    let source_blueprint_id = source_bundle
        .blueprint_id
        .as_ref()
        .expect("complete v2 source bundle has blueprint");
    let source_blueprint = state
        .artifact_repo
        .get_by_id(source_blueprint_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "Source plan blueprint not found: {}",
                source_blueprint_id.as_str()
            )
        })?;
    let cloned_blueprint =
        clone_plan_artifact_for_import(state, &source_blueprint, PLAN_REFERENCE_IMPORT_CREATOR)
            .await?;
    state
        .artifact_repo
        .add_relation(ArtifactRelation::related_to(
            cloned_artifact.id.clone(),
            cloned_blueprint.id.clone(),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let analysis = prepare_ideation_analysis_state_from_agent_workspace(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    let session = IdeationSession::builder()
        .project_id(workspace.project_id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .plan_artifact_id(cloned_artifact.id.clone())
        .plan_blueprint_artifact_id(cloned_blueprint.id.clone())
        .plan_contract_version(PLAN_CONTRACT_V2)
        .source_project_id(project.id.as_str())
        .source_session_id(source_session.id.as_str())
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .spawn_reason(PLAN_REFERENCE_IMPORT_REASON)
        .analysis(analysis)
        .build();
    let session = hydrate_agent_conversation_planning_session_title(state, session)
        .await
        .map_err(|error| error.to_string())?;
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .map_err(|error| error.to_string())?;

    workspace.linked_ideation_session_id = Some(session.id.clone());
    workspace.linked_plan_branch_id = None;
    workspace.updated_at = chrono::Utc::now();

    Ok(AgentPlanReferenceImport {
        composer_references: vec![
            ComposerArtifactReference {
                artifact_id: cloned_artifact.id.as_str().to_string(),
                kind: "plan".to_string(),
                title: reference
                    .title
                    .clone()
                    .or_else(|| Some(source_artifact.name.clone())),
                session_id: Some(session.id.as_str().to_string()),
                version: Some(cloned_artifact.metadata.version),
                status: Some("draft".to_string()),
            },
            ComposerArtifactReference {
                artifact_id: cloned_blueprint.id.as_str().to_string(),
                kind: "plan_blueprint".to_string(),
                title: Some(cloned_blueprint.name.clone()),
                session_id: Some(session.id.as_str().to_string()),
                version: Some(cloned_blueprint.metadata.version),
                status: Some("draft".to_string()),
            },
        ],
    })
}

fn is_plan_reference(reference: &ComposerArtifactReference) -> bool {
    reference.kind.trim().eq_ignore_ascii_case("plan")
}

fn is_same_plan_reference(
    reference: &ComposerArtifactReference,
    source: &ComposerArtifactReference,
) -> bool {
    is_plan_reference(reference)
        && reference.artifact_id.trim() == source.artifact_id.trim()
        && reference.session_id.as_deref().map(str::trim)
            == source.session_id.as_deref().map(str::trim)
}

fn clean_required_value(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return Err(format!("{label} is invalid"));
    }
    Ok(value.to_string())
}

pub(crate) async fn clone_plan_artifact_for_import(
    state: &AppState,
    source: &Artifact,
    creator: &str,
) -> Result<Artifact, String> {
    let new_artifact = Artifact {
        id: ArtifactId::new(),
        artifact_type: source.artifact_type,
        name: source.name.clone(),
        content: source.content.clone(),
        metadata: ArtifactMetadata::new(creator).with_version(1),
        derived_from: vec![source.id.clone()],
        bucket_id: source.bucket_id.clone(),
        archived_at: None,
    };

    let created = state
        .artifact_repo
        .create(new_artifact)
        .await
        .map_err(|error| error.to_string())?;
    let relation = ArtifactRelation::derived_from(created.id.clone(), source.id.clone());
    if let Err(error) = state.artifact_repo.add_relation(relation).await {
        warn!(
            cloned_artifact_id = %created.id,
            source_artifact_id = %source.id,
            error = %error,
            "Failed to record derived_from relation for imported plan artifact"
        );
    }

    Ok(created)
}
