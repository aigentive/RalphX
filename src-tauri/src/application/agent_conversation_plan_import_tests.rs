use crate::application::agent_conversation_plan_import::{
    copy_agent_conversation_plan, import_agent_conversation_plan_markdown,
    AgentConversationMarkdownImportRequest, AgentConversationPlanCopyRequest,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactBucketId,
    ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactType, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, IdeationSessionStatus,
    Priority, Project, ProjectId, ProposalCategory, TaskProposal, VerificationGap,
    VerificationRoundSnapshot, VerificationRunSnapshot, VerificationStatus,
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
    seed_source_plan_with_status(
        state,
        project_id,
        v1_content,
        v2_content,
        IdeationSessionStatus::Active,
    )
    .await
}

async fn seed_source_plan_with_status(
    state: &AppState,
    project_id: &ProjectId,
    v1_content: &str,
    v2_content: &str,
    status: IdeationSessionStatus,
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
                .status(status)
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

#[derive(Debug, PartialEq)]
struct ProposalIsolationSnapshot {
    proposals: Vec<(
        String,
        String,
        String,
        i32,
        bool,
        Option<String>,
        Option<u32>,
    )>,
    dependencies: Vec<(String, String, Option<String>, String)>,
}

async fn seed_source_proposal_dependency(
    state: &AppState,
    session: &IdeationSession,
) -> ProposalIsolationSnapshot {
    let mut foundation = TaskProposal::new(
        session.id.clone(),
        "Source foundation",
        ProposalCategory::Feature,
        Priority::High,
    );
    foundation.description = Some("Must remain on source session".to_string());
    foundation.priority_score = 85;
    foundation.selected = true;
    foundation.sort_order = 1;
    foundation.plan_artifact_id = session.plan_artifact_id.clone();
    foundation.plan_version_at_creation = Some(2);
    let foundation = state.task_proposal_repo.create(foundation).await.unwrap();

    let mut dependent = TaskProposal::new(
        session.id.clone(),
        "Source dependent",
        ProposalCategory::Test,
        Priority::Medium,
    );
    dependent.priority_score = 65;
    dependent.sort_order = 2;
    let dependent = state.task_proposal_repo.create(dependent).await.unwrap();

    state
        .proposal_dependency_repo
        .add_dependency(
            &dependent.id,
            &foundation.id,
            Some("target copy must not rewrite source dependency graph"),
            Some("manual"),
        )
        .await
        .unwrap();

    proposal_isolation_snapshot(state, &session.id).await
}

async fn proposal_isolation_snapshot(
    state: &AppState,
    session_id: &crate::domain::entities::IdeationSessionId,
) -> ProposalIsolationSnapshot {
    let mut proposals = state
        .task_proposal_repo
        .get_by_session(session_id)
        .await
        .unwrap()
        .into_iter()
        .map(|proposal| {
            (
                proposal.id.as_str().to_string(),
                proposal.title,
                proposal.status.to_string(),
                proposal.priority_score,
                proposal.selected,
                proposal
                    .plan_artifact_id
                    .as_ref()
                    .map(|artifact_id| artifact_id.as_str().to_string()),
                proposal.plan_version_at_creation,
            )
        })
        .collect::<Vec<_>>();
    proposals.sort_by(|left, right| left.0.cmp(&right.0));

    let mut dependencies = state
        .proposal_dependency_repo
        .get_all_for_session_with_source(session_id)
        .await
        .unwrap()
        .into_iter()
        .map(|(proposal_id, depends_on_id, reason, source)| {
            (
                proposal_id.as_str().to_string(),
                depends_on_id.as_str().to_string(),
                reason,
                source,
            )
        })
        .collect::<Vec<_>>();
    dependencies.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    ProposalIsolationSnapshot {
        proposals,
        dependencies,
    }
}

async fn seed_source_verification_state(
    state: &AppState,
    session: &IdeationSession,
) -> VerificationRunSnapshot {
    state
        .ideation_session_repo
        .increment_verification_generation(&session.id)
        .await
        .unwrap();
    state
        .ideation_session_repo
        .update_verification_state(&session.id, VerificationStatus::NeedsRevision, false)
        .await
        .unwrap();

    let refreshed = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .unwrap();
    let gap = VerificationGap {
        severity: "high".to_string(),
        category: "correctness".to_string(),
        description: "Source verifier finding should stay on the source session".to_string(),
        why_it_matters: Some("Copy/import must not reset source verification state".to_string()),
        source: Some("review-regression".to_string()),
    };
    let snapshot = VerificationRunSnapshot {
        generation: refreshed.verification_generation,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 2,
        max_rounds: 4,
        best_round_index: Some(1),
        convergence_reason: Some("source still needs revision".to_string()),
        current_gaps: vec![gap.clone()],
        rounds: vec![VerificationRoundSnapshot {
            round: 2,
            gap_score: 3,
            fingerprints: vec!["source-verification-fingerprint".to_string()],
            gaps: vec![gap],
            parse_failed: false,
        }],
    };
    state
        .ideation_session_repo
        .save_verification_run_snapshot(&session.id, &snapshot)
        .await
        .unwrap();
    snapshot
}

fn content_text(artifact: &Artifact) -> &str {
    match &artifact.content {
        ArtifactContent::Inline { text } => text.as_str(),
        ArtifactContent::File { .. } => panic!("expected inline artifact content"),
    }
}

async fn assert_no_target_workspace(state: &AppState, conversation: &ChatConversation) {
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
    let source_proposal_snapshot = seed_source_proposal_dependency(&state, &source_session).await;
    let source_verification_snapshot =
        seed_source_verification_state(&state, &source_session).await;

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

    assert_eq!(
        proposal_isolation_snapshot(&state, &source_session.id).await,
        source_proposal_snapshot
    );
    assert!(state
        .task_proposal_repo
        .get_by_session(&target_session.id)
        .await
        .unwrap()
        .is_empty());
    let source_after_copy = state
        .ideation_session_repo
        .get_by_id(&source_session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        source_after_copy.verification_generation,
        source_verification_snapshot.generation
    );
    assert_eq!(
        source_after_copy.verification_status,
        VerificationStatus::NeedsRevision
    );
    assert_eq!(
        state
            .ideation_session_repo
            .get_verification_run_snapshot(
                &source_session.id,
                source_verification_snapshot.generation,
            )
            .await
            .unwrap(),
        Some(source_verification_snapshot)
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

#[tokio::test]
async fn copy_rejects_source_session_without_plan_before_target_mutation() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "source-without-plan").await;
    let conversation = seed_project_conversation(&state, &project.id).await;
    let source_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .status(IdeationSessionStatus::Active)
                .build(),
        )
        .await
        .unwrap();

    let err = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: "missing-plan-artifact".to_string(),
            source_version: 1,
        },
    )
    .await
    .unwrap_err();

    assert!(err.contains("does not have a plan artifact"));
    assert_no_target_workspace(&state, &conversation).await;
}

#[tokio::test]
async fn copy_rejects_missing_source_artifact_before_target_mutation() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "missing-source-artifact").await;
    let conversation = seed_project_conversation(&state, &project.id).await;
    let missing_artifact_id = ArtifactId::new();
    let source_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .status(IdeationSessionStatus::Active)
                .plan_artifact_id(missing_artifact_id.clone())
                .build(),
        )
        .await
        .unwrap();

    let err = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: missing_artifact_id.as_str().to_string(),
            source_version: 1,
        },
    )
    .await
    .unwrap_err();

    assert!(err.contains("Source plan version 1 was not found"));
    assert_no_target_workspace(&state, &conversation).await;
}

#[tokio::test]
async fn copy_rejects_archived_source_session_before_target_mutation() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "archived-source").await;
    let conversation = seed_project_conversation(&state, &project.id).await;
    let (source_session, _source_v1, source_v2) = seed_source_plan_with_status(
        &state,
        &project.id,
        "source v1",
        "source v2",
        IdeationSessionStatus::Archived,
    )
    .await;

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

    assert!(err.contains("archived source session"));
    assert_no_target_workspace(&state, &conversation).await;
}

#[tokio::test]
async fn copy_rejects_non_specification_source_artifact_before_target_mutation() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "non-spec-source").await;
    let conversation = seed_project_conversation(&state, &project.id).await;
    let mut source_artifact = plan_artifact("Research note", "not a plan", 1);
    source_artifact.artifact_type = ArtifactType::ResearchDocument;
    let source_artifact = state.artifact_repo.create(source_artifact).await.unwrap();
    let source_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .status(IdeationSessionStatus::Active)
                .plan_artifact_id(source_artifact.id.clone())
                .build(),
        )
        .await
        .unwrap();

    let err = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_artifact.id.as_str().to_string(),
            source_version: 1,
        },
    )
    .await
    .unwrap_err();

    assert!(err.contains("not a specification/plan type"));
    assert_no_target_workspace(&state, &conversation).await;
}

#[tokio::test]
async fn copy_rejects_zero_or_stale_source_version_before_target_mutation() {
    let state = AppState::new_sqlite_test();
    let project = seed_project(&state, "stale-source-version").await;
    let zero_version_conversation = seed_project_conversation(&state, &project.id).await;
    let stale_artifact_conversation = seed_project_conversation(&state, &project.id).await;
    let (source_session, source_v1, source_v2) =
        seed_source_plan(&state, &project.id, "source v1", "source v2").await;

    let zero_version_err = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: zero_version_conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 0,
        },
    )
    .await
    .unwrap_err();
    assert!(zero_version_err.contains("version is required"));
    assert_no_target_workspace(&state, &zero_version_conversation).await;

    let stale_artifact_err = copy_agent_conversation_plan(
        &state,
        AgentConversationPlanCopyRequest {
            conversation_id: stale_artifact_conversation.id.as_str(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v1.id.as_str().to_string(),
            source_version: 1,
        },
    )
    .await
    .unwrap_err();
    assert!(stale_artifact_err.contains("stale"));
    assert_no_target_workspace(&state, &stale_artifact_conversation).await;
}
