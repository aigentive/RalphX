use tracing::warn;

use crate::application::agent_planning_session_titles::hydrate_agent_conversation_planning_session_title;
use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::AppState;
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
    pub composer_reference: ComposerArtifactReference,
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

pub(crate) fn rewrite_imported_plan_reference(
    references: &[ComposerArtifactReference],
    source_reference: &ComposerArtifactReference,
    imported_reference: &ComposerArtifactReference,
) -> Vec<ComposerArtifactReference> {
    references
        .iter()
        .map(|reference| {
            if is_same_plan_reference(reference, source_reference) {
                imported_reference.clone()
            } else {
                reference.clone()
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
    if source_bundle.overview_id != source_artifact.id {
        return Err("Selected artifact is not the source session's current overview".to_string());
    }

    let cloned_artifact = clone_plan_artifact(state, &source_artifact).await?;
    let cloned_blueprint = if let Some(blueprint_id) = source_bundle.blueprint_id.as_ref() {
        let source_blueprint = state
            .artifact_repo
            .get_by_id(blueprint_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Source plan blueprint not found: {}", blueprint_id.as_str()))?;
        Some(clone_plan_artifact(state, &source_blueprint).await?)
    } else {
        None
    };
    if let Some(blueprint) = cloned_blueprint.as_ref() {
        state
            .artifact_repo
            .add_relation(ArtifactRelation::related_to(
                cloned_artifact.id.clone(),
                blueprint.id.clone(),
            ))
            .await
            .map_err(|error| error.to_string())?;
    }
    let analysis = prepare_ideation_analysis_state_from_agent_workspace(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    let session = IdeationSession::builder()
        .project_id(workspace.project_id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .plan_artifact_id(cloned_artifact.id.clone())
        .plan_contract_version(source_bundle.contract_version)
        .source_project_id(project.id.as_str())
        .source_session_id(source_session.id.as_str())
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .spawn_reason(PLAN_REFERENCE_IMPORT_REASON)
        .analysis(analysis)
        .build();
    let mut session = session;
    session.plan_blueprint_artifact_id = cloned_blueprint
        .as_ref()
        .map(|artifact| artifact.id.clone());
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
        composer_reference: ComposerArtifactReference {
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

async fn clone_plan_artifact(state: &AppState, source: &Artifact) -> Result<Artifact, String> {
    let new_artifact = Artifact {
        id: ArtifactId::new(),
        artifact_type: source.artifact_type,
        name: source.name.clone(),
        content: source.content.clone(),
        metadata: ArtifactMetadata::new(PLAN_REFERENCE_IMPORT_CREATOR).with_version(1),
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
            "Failed to record derived_from relation for agent plan-reference import"
        );
    }

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
        VerificationStatus,
    };
    use std::path::Path;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn setup_repo(root: &Path) -> String {
        std::fs::create_dir_all(root).expect("repo root should be created");
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test User"]);
        std::fs::write(root.join("README.md"), "hello\n").expect("fixture file should be written");
        git(root, &["add", "README.md"]);
        git(root, &["commit", "-m", "initial"]);
        git(root, &["rev-parse", "HEAD"])
    }

    fn reference(
        artifact_id: &ArtifactId,
        session_id: Option<&IdeationSessionId>,
    ) -> ComposerArtifactReference {
        ComposerArtifactReference {
            artifact_id: artifact_id.as_str().to_string(),
            kind: "plan".to_string(),
            title: Some("Source Plan".to_string()),
            session_id: session_id.map(|id| id.as_str().to_string()),
            version: Some(7),
            status: Some("accepted".to_string()),
        }
    }

    struct ImportFixture {
        _temp: tempfile::TempDir,
        state: AppState,
        project: Project,
        workspace: AgentConversationWorkspace,
        source_artifact: Artifact,
        source_session: IdeationSession,
    }

    async fn setup_import_fixture(label: &str) -> ImportFixture {
        setup_import_fixture_with_source_session(
            label,
            IdeationSessionStatus::Accepted,
            SessionPurpose::General,
        )
        .await
    }

    async fn setup_import_fixture_with_source_session(
        label: &str,
        status: IdeationSessionStatus,
        purpose: SessionPurpose,
    ) -> ImportFixture {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        let base_commit = setup_repo(&repo_path);

        let project_id = ProjectId::from_string(format!("project-plan-reference-import-{label}"));
        let mut project = Project::new(
            format!("Plan Reference Import {label}"),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

        let conversation_id =
            ChatConversationId::from_string(format!("conversation-plan-import-{label}"));
        let branch_name = format!("ralphx/test/plan-import-{label}");
        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path should resolve");
        std::fs::create_dir_all(workspace_path.parent().expect("workspace path should nest"))
            .expect("workspace parent should be created");
        let workspace_path_arg = workspace_path.to_string_lossy().to_string();
        git(
            &repo_path,
            &[
                "worktree",
                "add",
                "-b",
                branch_name.as_str(),
                workspace_path_arg.as_str(),
                "main",
            ],
        );

        let workspace = AgentConversationWorkspace::new(
            conversation_id,
            project_id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_commit),
            branch_name,
            workspace_path.to_string_lossy().to_string(),
        );

        let state = AppState::new_test();
        let source_artifact = state
            .artifact_repo
            .create(Artifact::new_inline(
                "Source Plan",
                ArtifactType::Specification,
                "# Source Plan\n\nUse this plan.",
                "test",
            ))
            .await
            .expect("source artifact should persist");
        let source_session = state
            .ideation_session_repo
            .create(
                IdeationSession::builder()
                    .project_id(project_id)
                    .title("Accepted source session")
                    .status(status)
                    .session_purpose(purpose)
                    .plan_artifact_id(source_artifact.id.clone())
                    .build(),
            )
            .await
            .expect("source session should persist");

        ImportFixture {
            _temp: temp,
            state,
            project,
            workspace,
            source_artifact,
            source_session,
        }
    }

    #[test]
    fn selected_plan_reference_rejects_multiple_plan_references() {
        let first = ComposerArtifactReference {
            artifact_id: "first".to_string(),
            kind: "plan".to_string(),
            title: None,
            session_id: Some("session-first".to_string()),
            version: None,
            status: None,
        };
        let mut second = first.clone();
        second.artifact_id = "second".to_string();

        let error = selected_plan_reference(&[first, second])
            .expect_err("multiple plan references should fail closed");

        assert!(error.contains("Multiple plan references"));
    }

    #[test]
    fn rewrite_imported_plan_reference_replaces_only_matching_plan() {
        let source = ComposerArtifactReference {
            artifact_id: "source-artifact".to_string(),
            kind: "PLAN".to_string(),
            title: Some("Source".to_string()),
            session_id: Some("source-session".to_string()),
            version: Some(1),
            status: None,
        };
        let imported = ComposerArtifactReference {
            artifact_id: "cloned-artifact".to_string(),
            kind: "plan".to_string(),
            title: Some("Clone".to_string()),
            session_id: Some("fresh-session".to_string()),
            version: Some(1),
            status: Some("draft".to_string()),
        };
        let unrelated = ComposerArtifactReference {
            artifact_id: "source-artifact".to_string(),
            kind: "issue".to_string(),
            title: None,
            session_id: Some("source-session".to_string()),
            version: None,
            status: None,
        };

        let rewritten = rewrite_imported_plan_reference(
            &[source.clone(), unrelated.clone()],
            &source,
            &imported,
        );

        assert_eq!(rewritten, vec![imported, unrelated]);
    }

    #[tokio::test]
    async fn import_plan_reference_clones_draft_session_without_source_state() {
        let mut fixture = setup_import_fixture("success").await;
        let import = import_agent_conversation_plan_reference(
            &fixture.state,
            &fixture.project,
            &mut fixture.workspace,
            &reference(
                &fixture.source_artifact.id,
                Some(&fixture.source_session.id),
            ),
        )
        .await
        .expect("plan reference import should succeed");

        let linked_session_id = fixture
            .workspace
            .linked_ideation_session_id
            .clone()
            .expect("workspace should link fresh session");
        let linked_session = fixture
            .state
            .ideation_session_repo
            .get_by_id(&linked_session_id)
            .await
            .expect("linked session lookup succeeds")
            .expect("linked session should exist");
        assert_eq!(linked_session.session_flow, IdeationSessionFlow::Planning);
        assert_eq!(
            linked_session.source_session_id.as_deref(),
            Some(fixture.source_session.id.as_str())
        );
        assert!(linked_session.parent_session_id.is_none());
        assert!(linked_session.inherited_plan_artifact_id.is_none());
        assert_eq!(
            linked_session.verification_status,
            VerificationStatus::Unverified
        );
        assert!(!linked_session.verification_in_progress);

        let cloned_artifact_id = linked_session
            .plan_artifact_id
            .as_ref()
            .expect("fresh session should point at cloned plan");
        assert_ne!(cloned_artifact_id, &fixture.source_artifact.id);
        let cloned_artifact = fixture
            .state
            .artifact_repo
            .get_by_id(cloned_artifact_id)
            .await
            .expect("cloned artifact lookup succeeds")
            .expect("cloned artifact should exist");
        assert_eq!(cloned_artifact.content, fixture.source_artifact.content);
        assert_eq!(cloned_artifact.metadata.version, 1);
        assert_eq!(
            cloned_artifact.metadata.created_by,
            PLAN_REFERENCE_IMPORT_CREATOR
        );
        assert!(cloned_artifact
            .derived_from
            .contains(&fixture.source_artifact.id));

        assert_eq!(import.composer_reference.kind, "plan");
        assert_eq!(
            import.composer_reference.artifact_id,
            cloned_artifact_id.as_str()
        );
        assert_eq!(
            import.composer_reference.session_id.as_deref(),
            Some(linked_session.id.as_str())
        );
        assert_eq!(import.composer_reference.status.as_deref(), Some("draft"));
    }

    #[tokio::test]
    async fn import_plan_reference_rejects_missing_session_id() {
        let mut fixture = setup_import_fixture("missing-session").await;
        let error = import_agent_conversation_plan_reference(
            &fixture.state,
            &fixture.project,
            &mut fixture.workspace,
            &reference(&fixture.source_artifact.id, None),
        )
        .await
        .expect_err("missing session id should fail");

        assert!(error.contains("missing session_id"));
        assert!(fixture.workspace.linked_ideation_session_id.is_none());
    }

    #[tokio::test]
    async fn import_plan_reference_rejects_non_spec_artifact() {
        let mut fixture = setup_import_fixture("non-spec").await;
        let notes = fixture
            .state
            .artifact_repo
            .create(Artifact::new_inline(
                "Notes",
                ArtifactType::ResearchDocument,
                "not a plan",
                "test",
            ))
            .await
            .expect("notes artifact should persist");

        let error = import_agent_conversation_plan_reference(
            &fixture.state,
            &fixture.project,
            &mut fixture.workspace,
            &reference(&notes.id, Some(&fixture.source_session.id)),
        )
        .await
        .expect_err("non-spec artifact should fail");

        assert!(error.contains("not a specification/plan"));
        assert!(fixture.workspace.linked_ideation_session_id.is_none());
    }

    #[tokio::test]
    async fn import_plan_reference_rejects_archived_and_verification_sessions() {
        let mut archived = setup_import_fixture_with_source_session(
            "archived",
            IdeationSessionStatus::Archived,
            SessionPurpose::General,
        )
        .await;
        let archived_error = import_agent_conversation_plan_reference(
            &archived.state,
            &archived.project,
            &mut archived.workspace,
            &reference(
                &archived.source_artifact.id,
                Some(&archived.source_session.id),
            ),
        )
        .await
        .expect_err("archived session should fail");
        assert!(archived_error.contains("archived session"));

        let mut verification = setup_import_fixture_with_source_session(
            "verification",
            IdeationSessionStatus::Accepted,
            SessionPurpose::Verification,
        )
        .await;
        let verification_error = import_agent_conversation_plan_reference(
            &verification.state,
            &verification.project,
            &mut verification.workspace,
            &reference(
                &verification.source_artifact.id,
                Some(&verification.source_session.id),
            ),
        )
        .await
        .expect_err("verification session should fail");
        assert!(verification_error.contains("verification child session"));
    }
}
