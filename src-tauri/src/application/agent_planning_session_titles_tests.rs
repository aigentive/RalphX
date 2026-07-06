use crate::application::agent_planning_session_titles::{
    hydrate_agent_conversation_planning_session_title,
    hydrate_agent_conversation_planning_session_titles,
    sync_linked_planning_session_title_from_conversation,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow,
    ProjectId,
};

#[tokio::test]
async fn hydrates_agent_planning_session_title_from_conversation_title() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-title-hydration".to_string());
    let conversation_id = ChatConversationId::from_string("11111111-2222-4333-8444-555555555555");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.title = Some("Review CLI gaps".to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let session = IdeationSession::builder()
        .project_id(project_id)
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(conversation_id.as_str())
        .build();

    let hydrated = hydrate_agent_conversation_planning_session_title(&state, session)
        .await
        .expect("planning session hydration succeeds");

    assert_eq!(hydrated.title.as_deref(), Some("Review CLI gaps"));
    assert_eq!(hydrated.title_source.as_deref(), Some("auto"));
}

#[tokio::test]
async fn hydration_keeps_user_named_planning_session_title() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-title-user".to_string());
    let conversation_id = ChatConversationId::from_string("22222222-2222-4333-8444-555555555555");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.title = Some("Conversation Title".to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let session = IdeationSession::builder()
        .project_id(project_id)
        .title("Custom Plan Session")
        .title_source("user")
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(conversation_id.as_str())
        .build();

    let hydrated = hydrate_agent_conversation_planning_session_title(&state, session)
        .await
        .expect("planning session hydration succeeds");

    assert_eq!(hydrated.title.as_deref(), Some("Custom Plan Session"));
    assert_eq!(hydrated.title_source.as_deref(), Some("user"));
}

#[tokio::test]
async fn hydration_ignores_non_agent_planning_sessions() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-title-non-agent".to_string());
    let conversation_id = ChatConversationId::from_string("22222222-3333-4333-8444-555555555555");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.title = Some("Conversation Title".to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let non_planning = IdeationSession::builder()
        .project_id(project_id.clone())
        .session_flow(IdeationSessionFlow::Ideation)
        .source_context_type("agent_conversation")
        .source_context_id(conversation_id.as_str())
        .build();
    let non_agent = IdeationSession::builder()
        .project_id(project_id)
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("project_chat")
        .source_context_id(conversation_id.as_str())
        .build();

    let hydrated =
        hydrate_agent_conversation_planning_session_titles(&state, vec![non_planning, non_agent])
            .await
            .expect("planning session hydration succeeds");

    assert_eq!(hydrated.len(), 2);
    assert!(hydrated.iter().all(|session| session.title.is_none()));
    assert!(hydrated
        .iter()
        .all(|session| session.title_source.as_deref() != Some("auto")));
}

#[tokio::test]
async fn hydration_skips_blank_conversation_titles() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-title-blank".to_string());
    let conversation_id = ChatConversationId::from_string("22222222-4444-4333-8444-555555555555");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.title = Some("   ".to_string());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let session = IdeationSession::builder()
        .project_id(project_id)
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(conversation_id.as_str())
        .build();

    let hydrated = hydrate_agent_conversation_planning_session_title(&state, session)
        .await
        .expect("planning session hydration succeeds");

    assert!(hydrated.title.is_none());
    assert!(hydrated.title_source.is_none());
}

#[tokio::test]
async fn syncs_linked_planning_session_title_when_conversation_is_named() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-title-sync".to_string());
    let conversation_id = ChatConversationId::from_string("33333333-2222-4333-8444-555555555555");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project_id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation_id.as_str())
                .build(),
        )
        .await
        .expect("planning session persisted");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-title-sync".to_string(),
        "/tmp/ralphx-agent-title-sync".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");

    let synced_session_id = sync_linked_planning_session_title_from_conversation(
        &state,
        &conversation_id,
        "Review CLI gaps",
    )
    .await
    .expect("title sync succeeds");

    assert_eq!(synced_session_id.as_ref(), Some(&session.id));
    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("planning session lookup succeeds")
        .expect("planning session exists");
    assert_eq!(updated.title.as_deref(), Some("Review CLI gaps"));
    assert_eq!(updated.title_source.as_deref(), Some("auto"));
}

#[tokio::test]
async fn sync_skips_blank_titles_missing_workspaces_and_unlinked_workspaces() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-title-sync-skips".to_string());
    let conversation_id = ChatConversationId::from_string("44444444-2222-4333-8444-555555555555");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let blank_title_result =
        sync_linked_planning_session_title_from_conversation(&state, &conversation_id, "   ")
            .await
            .expect("blank title sync succeeds");
    assert!(blank_title_result.is_none());

    let missing_workspace_result = sync_linked_planning_session_title_from_conversation(
        &state,
        &conversation_id,
        "Conversation title",
    )
    .await
    .expect("missing workspace sync succeeds");
    assert!(missing_workspace_result.is_none());

    let workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-title-sync-skip".to_string(),
        "/tmp/ralphx-agent-title-sync-skip".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");

    let unlinked_workspace_result = sync_linked_planning_session_title_from_conversation(
        &state,
        &conversation_id,
        "Conversation title",
    )
    .await
    .expect("unlinked workspace sync succeeds");
    assert!(unlinked_workspace_result.is_none());
}
