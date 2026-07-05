use crate::application::agent_conversation_plan_import::{
    copy_agent_conversation_plan, import_agent_conversation_plan_markdown,
    AgentConversationMarkdownImportRequest, AgentConversationPlanCopyRequest,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactBucketId,
    ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactType, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, IdeationSessionStatus,
    Project, ProjectId,
};

async fn seed_project(state: &AppState, name: &str) -> Project {
    let mut project = Project::new(name.to_string(), format!("/tmp/ralphx-tests/{name}"));
    project.base_branch = Some("main".to_string());
    state.project_repo.create(project).await.unwrap()
}

async fn seed_project_conversation(state: &AppState, project_id: &ProjectId) -> ChatConversation {
    state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .unwrap()
}

fn plan_artifact(name: &str, content: &str, version: u32) -> Artifact {
    Artifact {
        id: ArtifactId::new(),
        artifact_type: ArtifactType::Specification,
        name: name.to_string(),
        content: ArtifactContent::inline(content),
        metadata: ArtifactMetadata::new("test").with_version(version),
        derived_from: vec![],
        bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
        archived_at: None,
    }
}

async fn seed_source_plan(
    state: &AppState,
    project_id: &ProjectId,
    v1_content: &str,
    v2_content: &str,
) -> (IdeationSession, Artifact, Artifact) {
    let v1 = state
        .artifact_repo
        .create(plan_artifact("Source plan", v1_content, 1))
        .await
        .unwrap();
    let v2 = state
        .artifact_repo
        .create_with_previous_version(plan_artifact("Source plan", v2_content, 2), v1.id.clone())
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project_id.clone())
                .status(IdeationSessionStatus::Active)
                .plan_artifact_id(v2.id.clone())
                .build(),
        )
        .await
        .unwrap();
    (session, v1, v2)
}

async fn insert_approved_plan_row(
    state: &AppState,
    session_id: String,
    artifact_id: String,
    version: u32,
) {
    state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO plan_artifact_approvals (
                    session_id, artifact_id, artifact_version, status, approved_at, approved_by
                 ) VALUES (?1, ?2, ?3, 'approved', ?4, 'user')",
                rusqlite::params![
                    session_id,
                    artifact_id,
                    i64::from(version),
                    chrono::Utc::now().to_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

async fn approval_count(state: &AppState, session_id: String) -> i64 {
    state
        .db
        .run(move |conn| {
            let count = conn.query_row(
                "SELECT COUNT(*) FROM plan_artifact_approvals WHERE session_id = ?1",
                [session_id.as_str()],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap()
}

fn content_text(artifact: &Artifact) -> &str {
    match &artifact.content {
        ArtifactContent::Inline { text } => text.as_str(),
        ArtifactContent::File { .. } => panic!("expected inline artifact content"),
    }
}

#[tokio::test]
async fn copy_into_conversation_without_workspace_creates_planning_session_and_draft_plan() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "copy-no-workspace").await;
    let conversation = seed_project_conversation(&state, &project.id).await;
    let (source_session, _source_v1, source_v2) =
        seed_source_plan(&state, &project.id, "source v1", "source v2").await;
    insert_approved_plan_row(
        &state,
        source_session.id.as_str().to_string(),
        source_v2.id.as_str().to_string(),
        source_v2.metadata.version,
    )
    .await;

    let response = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 2,
        },
    )
    .await
    .unwrap();

    assert_eq!(response.conversation_id, conversation.id.as_str());
    assert_eq!(response.project_id, project.id.as_str());
    assert_eq!(
        response.source_artifact_id.as_deref(),
        Some(source_v2.id.as_str())
    );
    assert_eq!(response.source_version, Some(2));

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .expect("workspace should be created");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    assert_eq!(
        workspace
            .linked_ideation_session_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(response.planning_session_id.as_str())
    );

    let target_session = state
        .ideation_session_repo
        .get_by_id(&workspace.linked_ideation_session_id.clone().unwrap())
        .await
        .unwrap()
        .expect("linked planning session should exist");
    assert_eq!(target_session.session_flow, IdeationSessionFlow::Planning);
    assert_eq!(
        target_session.source_context_type.as_deref(),
        Some("agent_conversation")
    );
    assert_eq!(
        target_session.source_context_id.as_deref(),
        Some(response.conversation_id.as_str())
    );
    assert_eq!(
        target_session
            .plan_artifact_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(response.plan_artifact_id.as_str())
    );

    let copied = state
        .artifact_repo
        .get_by_id(&ArtifactId::from_string(&response.plan_artifact_id))
        .await
        .unwrap()
        .expect("copied artifact should exist");
    assert_eq!(copied.metadata.version, 1);
    assert_eq!(response.plan_artifact_version, 1);
    assert_eq!(content_text(&copied), "source v2");
    let lineage = state
        .artifact_repo
        .get_derived_from(&copied.id)
        .await
        .unwrap();
    assert_eq!(lineage.len(), 1);
    assert_eq!(lineage[0].id, source_v2.id);

    assert_eq!(
        approval_count(&state, source_session.id.as_str().to_string()).await,
        1
    );
    assert_eq!(
        approval_count(&state, response.planning_session_id).await,
        0
    );
}

#[tokio::test]
async fn copy_historical_version_over_existing_target_plan_preserves_target_history() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "copy-existing-plan").await;
    let conversation = seed_project_conversation(&state, &project.id).await;
    let (source_session, source_v1, source_v2) =
        seed_source_plan(&state, &project.id, "source v1", "source v2").await;
    let target_v1 = state
        .artifact_repo
        .create(plan_artifact("Target plan", "target v1", 1))
        .await
        .unwrap();
    let target_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation.id.as_str())
                .plan_artifact_id(target_v1.id.clone())
                .build(),
        )
        .await
        .unwrap();
    state
        .agent_conversation_workspace_repo
        .create_or_update(AgentConversationWorkspace::new(
            conversation.id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Plan,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            None,
            "ralphx/test/task-existing".to_string(),
            "/tmp/ralphx-tests/copy-existing-plan-worktree".to_string(),
        ))
        .await
        .unwrap();
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.linked_ideation_session_id = Some(target_session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let response = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 1,
        },
    )
    .await
    .unwrap();

    let copied = state
        .artifact_repo
        .get_by_id(&ArtifactId::from_string(&response.plan_artifact_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(content_text(&copied), "source v1");
    assert_eq!(copied.metadata.version, 2);
    let history = state
        .artifact_repo
        .get_version_history(&copied.id)
        .await
        .unwrap();
    assert_eq!(
        history.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
        vec![copied.id.as_str(), target_v1.id.as_str(),]
    );
    let lineage = state
        .artifact_repo
        .get_derived_from(&copied.id)
        .await
        .unwrap();
    assert_eq!(lineage.len(), 1);
    assert_eq!(lineage[0].id, source_v1.id);
    let source_latest = state
        .artifact_repo
        .get_by_id(&source_v2.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(content_text(&source_latest), "source v2");
}

#[tokio::test]
async fn markdown_import_uses_frontend_supplied_title_and_content_without_source_lineage() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "markdown-import").await;
    let conversation = seed_project_conversation(&state, &project.id).await;

    let response = import_agent_conversation_plan_markdown(
        &state,
        AgentConversationMarkdownImportRequest {
            conversation_id: conversation.id.as_str(),
            title: "Imported plan.md".to_string(),
            content: "# Imported\n\nPlan body".to_string(),
        },
    )
    .await
    .unwrap();

    let imported = state
        .artifact_repo
        .get_by_id(&ArtifactId::from_string(&response.plan_artifact_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(imported.name, "Imported plan.md");
    assert_eq!(content_text(&imported), "# Imported\n\nPlan body");
    assert!(imported.content.is_inline());
    assert!(state
        .artifact_repo
        .get_derived_from(&imported.id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        approval_count(&state, response.planning_session_id).await,
        0
    );
}

#[tokio::test]
async fn copy_rejects_workspace_linked_to_unrelated_planning_session() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "stale-link-target").await;
    let other_project = seed_project(&state, "stale-link-other").await;
    let conversation = seed_project_conversation(&state, &project.id).await;
    let (source_session, _source_v1, source_v2) =
        seed_source_plan(&state, &project.id, "source v1", "source v2").await;
    let unrelated_plan = state
        .artifact_repo
        .create(plan_artifact("Unrelated plan", "unrelated", 1))
        .await
        .unwrap();
    let unrelated_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(other_project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id("other-conversation")
                .plan_artifact_id(unrelated_plan.id.clone())
                .build(),
        )
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test/stale-link".to_string(),
        "/tmp/ralphx-tests/stale-link-worktree".to_string(),
    );
    workspace.linked_ideation_session_id = Some(unrelated_session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let err = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 2,
        },
    )
    .await
    .unwrap_err();

    assert!(err.contains("does not belong"));
    let unchanged_session = state
        .ideation_session_repo
        .get_by_id(&unrelated_session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        unchanged_session
            .plan_artifact_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(unrelated_plan.id.as_str())
    );
}

#[tokio::test]
async fn copy_rejects_project_mismatch_without_creating_target_workspace() {
    let state = AppState::new_sqlite_test();
    let target_project = seed_project(&state, "target-project").await;
    let other_project = seed_project(&state, "other-project").await;
    let conversation = seed_project_conversation(&state, &target_project.id).await;
    let (source_session, _source_v1, source_v2) =
        seed_source_plan(&state, &other_project.id, "source v1", "source v2").await;

    let err = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 2,
        },
    )
    .await
    .unwrap_err();

    assert!(err.contains("different project"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .is_none());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conversation.agent_mode, None);
}
