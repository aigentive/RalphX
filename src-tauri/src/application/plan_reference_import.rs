use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, Artifact, ArtifactId, ArtifactMetadata, ArtifactRelation,
    IdeationSession, IdeationSessionFlow, IdeationSessionId, Project,
};
use crate::domain::services::ComposerArtifactReference;

const PLAN_REFERENCE_KIND: &str = "plan";
const AGENT_PLAN_REFERENCE_IMPORT_CREATOR: &str = "agent_plan_reference_import";
const AGENT_PLAN_REFERENCE_IMPORT_REASON: &str = "agent_plan_reference_import";
const AGENT_CONVERSATION_SOURCE_CONTEXT: &str = "agent_conversation";
const EXTERNAL_PLAN_REFERENCE_IMPORT_CREATOR: &str = "plan_import";

#[derive(Clone, Copy)]
enum PlanReferenceArtifactRelationFailurePolicy {
    FailImport,
    WarnAndContinue,
}

pub(crate) struct PlanReferenceArtifactCloneOptions {
    created_by: &'static str,
    relation_failure_policy: PlanReferenceArtifactRelationFailurePolicy,
}

impl PlanReferenceArtifactCloneOptions {
    pub(crate) fn agent_conversation_import() -> Self {
        Self {
            created_by: AGENT_PLAN_REFERENCE_IMPORT_CREATOR,
            relation_failure_policy: PlanReferenceArtifactRelationFailurePolicy::FailImport,
        }
    }

    pub(crate) fn external_ideation_import() -> Self {
        Self {
            created_by: EXTERNAL_PLAN_REFERENCE_IMPORT_CREATOR,
            relation_failure_policy: PlanReferenceArtifactRelationFailurePolicy::WarnAndContinue,
        }
    }
}

fn is_plan_reference(reference: &ComposerArtifactReference) -> bool {
    reference
        .kind
        .trim()
        .eq_ignore_ascii_case(PLAN_REFERENCE_KIND)
}

fn selected_plan_reference(
    references: &[ComposerArtifactReference],
) -> Result<Option<&ComposerArtifactReference>, String> {
    let mut plan_references = references
        .iter()
        .filter(|reference| is_plan_reference(reference));
    let selected = plan_references.next();
    if plan_references.next().is_some() {
        return Err(
            "Agent conversation start requires exactly one plan reference; remove extra selected plans"
                .to_string(),
        );
    }
    Ok(selected)
}

pub(crate) fn selected_plan_reference_requires_workspace(
    references: &[ComposerArtifactReference],
) -> Result<bool, String> {
    selected_plan_reference(references).map(|reference| reference.is_some())
}

pub(crate) async fn import_selected_plan_reference_for_agent_start(
    state: &AppState,
    project: &Project,
    workspace: &mut AgentConversationWorkspace,
    references: &[ComposerArtifactReference],
) -> Result<Vec<ComposerArtifactReference>, String> {
    let Some(reference) = selected_plan_reference(references)? else {
        return Ok(references.to_vec());
    };

    if workspace.linked_ideation_session_id.is_some() {
        return Ok(references.to_vec());
    }

    let source_artifact_id = ArtifactId::from_string(reference.artifact_id.clone());
    let source_artifact = state
        .artifact_repo
        .get_by_id(&source_artifact_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Plan artifact not found: {}", reference.artifact_id))?;

    let source_session_id = reference
        .session_id
        .as_deref()
        .map(IdeationSessionId::from_string);
    let source_session = match source_session_id.as_ref() {
        Some(session_id) => state
            .ideation_session_repo
            .get_by_id(session_id)
            .await
            .map_err(|error| error.to_string())?,
        None => None,
    };

    let cloned_artifact = clone_plan_reference_artifact(
        state,
        &source_artifact,
        PlanReferenceArtifactCloneOptions::agent_conversation_import(),
    )
    .await?;
    let analysis = prepare_ideation_analysis_state_from_agent_workspace(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    let fresh_session = create_fresh_plan_reference_session(
        state,
        project,
        workspace,
        source_session.as_ref().map(|session| &session.id),
        source_session
            .as_ref()
            .map(|session| session.project_id.as_str().to_string()),
        cloned_artifact.id.clone(),
        source_artifact.name.clone(),
        analysis,
    )
    .await?;

    workspace.linked_ideation_session_id = Some(fresh_session.id.clone());
    workspace.linked_plan_branch_id = None;
    workspace.updated_at = chrono::Utc::now();

    Ok(rewrite_active_plan_reference(
        references,
        &source_artifact_id,
        &fresh_session.id,
        &cloned_artifact,
    ))
}

pub(crate) async fn clone_plan_reference_artifact(
    state: &AppState,
    source: &Artifact,
    options: PlanReferenceArtifactCloneOptions,
) -> Result<Artifact, String> {
    let cloned = Artifact {
        id: ArtifactId::new(),
        artifact_type: source.artifact_type,
        name: source.name.clone(),
        content: source.content.clone(),
        metadata: ArtifactMetadata::new(options.created_by).with_version(1),
        derived_from: vec![source.id.clone()],
        bucket_id: source.bucket_id.clone(),
        archived_at: None,
    };
    let cloned = state
        .artifact_repo
        .create(cloned)
        .await
        .map_err(|error| error.to_string())?;
    let relation_result = state
        .artifact_repo
        .add_relation(ArtifactRelation::derived_from(
            cloned.id.clone(),
            source.id.clone(),
        ))
        .await;
    if let Err(error) = relation_result {
        match options.relation_failure_policy {
            PlanReferenceArtifactRelationFailurePolicy::FailImport => {
                return Err(error.to_string());
            }
            PlanReferenceArtifactRelationFailurePolicy::WarnAndContinue => {
                tracing::warn!(
                    "Failed to record derived_from relation for cloned plan artifact: {}",
                    error
                );
            }
        }
    }
    Ok(cloned)
}

async fn create_fresh_plan_reference_session(
    state: &AppState,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    source_session_id: Option<&IdeationSessionId>,
    source_project_id: Option<String>,
    cloned_artifact_id: ArtifactId,
    plan_title: String,
    analysis: crate::domain::entities::IdeationAnalysisState,
) -> Result<IdeationSession, String> {
    let mut builder = IdeationSession::builder()
        .project_id(project.id.clone())
        .title(plan_title)
        .plan_artifact_id(cloned_artifact_id)
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type(AGENT_CONVERSATION_SOURCE_CONTEXT)
        .source_context_id(workspace.conversation_id.as_str())
        .spawn_reason(AGENT_PLAN_REFERENCE_IMPORT_REASON)
        .analysis(analysis);

    if let Some(source_session_id) = source_session_id {
        builder = builder.source_session_id(source_session_id.as_str());
    }
    if let Some(source_project_id) = source_project_id {
        builder = builder.source_project_id(source_project_id);
    }

    state
        .ideation_session_repo
        .create(builder.build())
        .await
        .map_err(|error| error.to_string())
}

fn rewrite_active_plan_reference(
    references: &[ComposerArtifactReference],
    source_artifact_id: &ArtifactId,
    fresh_session_id: &IdeationSessionId,
    cloned_artifact: &Artifact,
) -> Vec<ComposerArtifactReference> {
    references
        .iter()
        .map(|reference| {
            if is_plan_reference(reference) && reference.artifact_id == source_artifact_id.as_str()
            {
                ComposerArtifactReference {
                    artifact_id: cloned_artifact.id.as_str().to_string(),
                    kind: PLAN_REFERENCE_KIND.to_string(),
                    title: Some(cloned_artifact.name.clone()),
                    session_id: Some(fresh_session_id.as_str().to_string()),
                    version: Some(cloned_artifact.metadata.version),
                    status: Some("active".to_string()),
                }
            } else {
                reference.clone()
            }
        })
        .collect()
}
