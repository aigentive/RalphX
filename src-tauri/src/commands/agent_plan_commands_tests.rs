use super::agent_plan_commands::{
    copy_agent_conversation_plan_for_state, import_agent_conversation_plan_for_state,
    CopyAgentConversationPlanInput, ImportAgentConversationPlanInput,
};
use crate::application::{
    agent_conversation_workspace::resolve_agent_conversation_workspace_path, AppState,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactBucketId,
    ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactRelationType, ArtifactType,
    ChatConversation, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow,
    IdeationSessionStatus, Project,
};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo(root: &Path) {
    std::fs::create_dir_all(root).expect("repo root should be created");
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    std::fs::write(root.join("README.md"), "hello\n").expect("fixture file should be written");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

fn inline_plan(name: &str, content: &str, version: u32) -> Artifact {
    Artifact {
        id: ArtifactId::new(),
        artifact_type: ArtifactType::Specification,
        name: name.to_string(),
        content: ArtifactContent::inline(content),
        metadata: ArtifactMetadata::new("orchestrator").with_version(version),
        derived_from: vec![],
        bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
        archived_at: None,
    }
}

fn file_plan(name: &str, path: &str, version: u32) -> Artifact {
    Artifact {
        id: ArtifactId::new(),
        artifact_type: ArtifactType::Specification,
        name: name.to_string(),
        content: ArtifactContent::File {
            path: path.to_string(),
        },
        metadata: ArtifactMetadata::new("orchestrator").with_version(version),
        derived_from: vec![],
        bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
        archived_at: None,
    }
}

async fn setup_target_workspace(
    mode: AgentConversationWorkspaceMode,
) -> (AppState, Project, ChatConversation, TempDir) {
    let state = AppState::new_sqlite_test();
    let test_root = tempfile::tempdir().expect("test root should be created");
    let project_dir = test_root.path().join("project");
    let worktree_parent = test_root.path().join("worktrees");
    std::fs::create_dir_all(&worktree_parent).unwrap();
    setup_repo(&project_dir);
    let mut project = Project::new(
        "Agent plan test".to_string(),
        project_dir.to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().into_owned());
    let project = state.project_repo.create(project).await.unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(mode));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation.id).unwrap();
    std::fs::create_dir_all(workspace_path.parent().unwrap()).unwrap();
    let workspace_path_arg = workspace_path.to_string_lossy().to_string();
    git(
        &project_dir,
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/test/agent-plan",
            workspace_path_arg.as_str(),
            "main",
        ],
    );
    let workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/agent-plan".to_string(),
        workspace_path.to_string_lossy().into_owned(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    (state, project, conversation, test_root)
}

async fn seed_source_plan(
    state: &AppState,
    project: &Project,
) -> (IdeationSession, Artifact, Artifact) {
    let source_v1 = state
        .artifact_repo
        .create(inline_plan("Source plan", "# Source v1", 1))
        .await
        .unwrap();
    let source_v2 = state
        .artifact_repo
        .create_with_previous_version(
            Artifact {
                id: ArtifactId::new(),
                artifact_type: ArtifactType::Specification,
                name: "Source plan".to_string(),
                content: ArtifactContent::inline("# Source v2"),
                metadata: ArtifactMetadata::new("orchestrator").with_version(2),
                derived_from: vec![],
                bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
                archived_at: None,
            },
            source_v1.id.clone(),
        )
        .await
        .unwrap();
    let source_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .plan_artifact_id(source_v2.id.clone())
                .build(),
        )
        .await
        .unwrap();
    (source_session, source_v1, source_v2)
}

#[tokio::test]
async fn import_agent_conversation_plan_switches_to_plan_and_creates_draft() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;

    let response = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Imported plan".to_string(),
            content: "# Imported plan".to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.workspace.mode, "plan");
    assert_eq!(response.artifact.name, "Imported plan");
    assert_eq!(response.artifact.content, "# Imported plan");
    assert_eq!(response.artifact.version, 1);
    assert_eq!(
        response.artifact.plan_approval_status.as_deref(),
        Some("draft")
    );

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    let linked_session_id = workspace.linked_ideation_session_id.unwrap();
    assert_eq!(linked_session_id.as_str(), response.session_id);

    let session = state
        .ideation_session_repo
        .get_by_id(&linked_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
    assert_eq!(
        session.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(response.artifact.id.as_str()),
    );
}

#[tokio::test]
async fn import_agent_conversation_plan_rejects_blank_fields_before_switching() {
    let state = AppState::new_sqlite_test();

    let title_error = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: "conversation-blank-title".to_string(),
            title: "   ".to_string(),
            content: "# Plan".to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(title_error, "Plan title is required");

    let content_error = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: "conversation-blank-content".to_string(),
            title: "Imported plan".to_string(),
            content: "\n\t ".to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(content_error, "Plan content is required");
}

#[tokio::test]
async fn copy_agent_conversation_plan_rejects_zero_source_version_before_lookup() {
    let state = AppState::new_sqlite_test();

    let error = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: "conversation-zero-version".to_string(),
            source_session_id: "source-session".to_string(),
            source_artifact_id: "source-artifact".to_string(),
            source_version: 0,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error, "Source plan version must be greater than zero");
}

#[tokio::test]
async fn copy_agent_conversation_plan_uses_selected_source_version() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let (source_session, source_v1, source_v2) = seed_source_plan(&state, &project).await;

    let response = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 1,
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.artifact.content, "# Source v1");
    assert_eq!(
        response.artifact.derived_from,
        vec![source_v1.id.as_str().to_string()]
    );
    assert_eq!(
        response.artifact.plan_approval_status.as_deref(),
        Some("draft")
    );

    let source_session_after = state
        .ideation_session_repo
        .get_by_id(&source_session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source_session_after.plan_artifact_id, Some(source_v2.id));

    let relations = state
        .artifact_repo
        .get_relations(&ArtifactId::from_string(response.artifact.id.clone()))
        .await
        .unwrap();
    assert!(relations.iter().any(|relation| {
        relation.relation_type == ArtifactRelationType::DerivedFrom
            && relation.to_artifact_id == source_v1.id
    }));
}

#[tokio::test]
async fn copy_agent_conversation_plan_rejects_file_backed_source_plan() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let source_plan = state
        .artifact_repo
        .create(file_plan("File source plan", "/tmp/source-plan.md", 1))
        .await
        .unwrap();
    let source_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .plan_artifact_id(source_plan.id.clone())
                .build(),
        )
        .await
        .unwrap();

    let error = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_plan.id.as_str().to_string(),
            source_version: 1,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        "File-backed source plans cannot be copied from the agent Plan tab"
    );
}

#[tokio::test]
async fn copy_agent_conversation_plan_over_existing_target_adds_local_version() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Plan).await;
    let (source_session, _source_v1, source_v2) = seed_source_plan(&state, &project).await;

    let target_v1 = state
        .artifact_repo
        .create(inline_plan("Target plan", "# Target v1", 1))
        .await
        .unwrap();
    let mut target_session = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .plan_artifact_id(target_v1.id.clone())
        .source_context_type("agent_conversation")
        .source_context_id(conversation.id.as_str())
        .build();
    target_session = state
        .ideation_session_repo
        .create(target_session)
        .await
        .unwrap();
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    workspace.linked_ideation_session_id = Some(target_session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let response = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 2,
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.session_id, target_session.id.as_str());
    assert_eq!(response.artifact.content, "# Source v2");
    assert_eq!(response.artifact.version, 2);

    let refreshed_session = state
        .ideation_session_repo
        .get_by_id(&target_session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        refreshed_session
            .plan_artifact_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(response.artifact.id.as_str()),
    );

    let history = state
        .artifact_repo
        .get_version_history(&ArtifactId::from_string(response.artifact.id.clone()))
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version, 2);
    assert_eq!(history[1].id, target_v1.id);
}

#[tokio::test]
async fn import_agent_conversation_plan_rejects_accepted_target_session() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Plan).await;
    let target_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation.id.as_str())
                .build(),
        )
        .await
        .unwrap();
    state
        .ideation_session_repo
        .update_status(&target_session.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    workspace.linked_ideation_session_id = Some(target_session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let error = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Imported plan".to_string(),
            content: "# Imported plan".to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        "Validation error: Cannot modify accepted session. Reopen it first."
    );
}
