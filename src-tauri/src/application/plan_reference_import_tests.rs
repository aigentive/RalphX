use super::plan_reference_import::{
    import_agent_conversation_plan_reference, rewrite_imported_plan_references,
    selected_plan_reference,
};
use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::AppState;
use crate::domain::entities::ideation::{PLAN_CONTRACT_V1, PLAN_CONTRACT_V2};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactId, ArtifactType,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow,
    IdeationSessionId, IdeationSessionStatus, Project, ProjectId, SessionPurpose,
    VerificationStatus,
};
use crate::domain::services::ComposerArtifactReference;
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
        PLAN_CONTRACT_V2,
    )
    .await
}

async fn setup_import_fixture_with_source_session(
    label: &str,
    status: IdeationSessionStatus,
    purpose: SessionPurpose,
    contract_version: i32,
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
    let source_blueprint = if contract_version >= PLAN_CONTRACT_V2 {
        Some(
            state
                .artifact_repo
                .create(Artifact::new_inline(
                    "Source Blueprint",
                    ArtifactType::Specification,
                    "# Source Blueprint\n\nImplement the plan.",
                    "test",
                ))
                .await
                .expect("source blueprint should persist"),
        )
    } else {
        None
    };
    let mut source_builder = IdeationSession::builder()
        .project_id(project_id)
        .title("Accepted source session")
        .status(status)
        .session_purpose(purpose)
        .plan_artifact_id(source_artifact.id.clone())
        .plan_contract_version(contract_version);
    if let Some(blueprint) = source_blueprint.as_ref() {
        source_builder = source_builder.plan_blueprint_artifact_id(blueprint.id.clone());
    }
    let source_session = state
        .ideation_session_repo
        .create(source_builder.build())
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
fn rewrite_imported_plan_references_replaces_only_matching_plan_with_bundle() {
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
    let imported_blueprint = ComposerArtifactReference {
        artifact_id: "cloned-blueprint".to_string(),
        kind: "plan_blueprint".to_string(),
        title: Some("Clone Blueprint".to_string()),
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

    let rewritten = rewrite_imported_plan_references(
        &[source.clone(), unrelated.clone()],
        &source,
        &[imported.clone(), imported_blueprint.clone()],
    );

    assert_eq!(rewritten, vec![imported, imported_blueprint, unrelated]);
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
        "agent_plan_reference_import"
    );
    assert!(cloned_artifact
        .derived_from
        .contains(&fixture.source_artifact.id));
    let cloned_blueprint_id = linked_session
        .plan_blueprint_artifact_id
        .as_ref()
        .expect("fresh session should point at cloned blueprint");
    let source_blueprint = fixture
        .source_session
        .plan_blueprint_artifact_id
        .as_ref()
        .expect("complete source should have blueprint");
    assert_ne!(cloned_blueprint_id, source_blueprint);
    assert_eq!(linked_session.plan_contract_version, PLAN_CONTRACT_V2);

    assert_eq!(import.composer_references.len(), 2);
    assert_eq!(import.composer_references[0].kind, "plan");
    assert_eq!(
        import.composer_references[0].artifact_id,
        cloned_artifact_id.as_str()
    );
    assert_eq!(
        import.composer_references[0].session_id.as_deref(),
        Some(linked_session.id.as_str())
    );
    assert_eq!(
        import.composer_references[0].status.as_deref(),
        Some("draft")
    );
    assert_eq!(import.composer_references[1].kind, "plan_blueprint");
    assert_eq!(
        import.composer_references[1].artifact_id,
        cloned_blueprint_id.as_str()
    );
    assert_eq!(
        import.composer_references[1].session_id.as_deref(),
        Some(linked_session.id.as_str())
    );
}

#[tokio::test]
async fn import_plan_reference_rejects_legacy_source_before_creating_edit_session() {
    let mut fixture = setup_import_fixture_with_source_session(
        "legacy",
        IdeationSessionStatus::Accepted,
        SessionPurpose::General,
        PLAN_CONTRACT_V1,
    )
    .await;

    let error = import_agent_conversation_plan_reference(
        &fixture.state,
        &fixture.project,
        &mut fixture.workspace,
        &reference(
            &fixture.source_artifact.id,
            Some(&fixture.source_session.id),
        ),
    )
    .await
    .expect_err("legacy plans must not create a new Edit session");

    assert!(error.contains("predates implementation blueprints"));
    assert!(fixture.workspace.linked_ideation_session_id.is_none());
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
        PLAN_CONTRACT_V2,
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
        PLAN_CONTRACT_V2,
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
