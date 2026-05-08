use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrDescription, ChatConversationId, IdeationAnalysisBaseRefKind, PlanBranchId,
    ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::testing::SqliteTestDb;

use super::SqliteAgentConversationWorkspaceRepository;

fn setup_repo() -> (
    SqliteTestDb,
    SqliteAgentConversationWorkspaceRepository,
    ChatConversationId,
) {
    let db = SqliteTestDb::new("sqlite_agent_conversation_workspace_repo_tests");
    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    seed_conversation(&db, &conversation_id);
    let repo = SqliteAgentConversationWorkspaceRepository::from_shared(db.shared_conn());
    (db, repo, conversation_id)
}

fn seed_conversation(db: &SqliteTestDb, conversation_id: &ChatConversationId) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (
                id, context_type, context_id, title, message_count, created_at, updated_at
             ) VALUES (
                ?1, 'project', 'project-1', 'Workspace chat', 0,
                '2026-04-26T09:00:00Z', '2026-04-26T09:00:00Z'
             )",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });
}

fn make_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-11111111".to_string(),
        "/tmp/ralphx/agent-11111111".to_string(),
    )
}

#[tokio::test]
async fn pr_description_round_trips_and_clears() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();

    repo.save_pr_description(
        &conversation_id,
        AgentWorkspacePrDescription::new(
            Some("Describe agent workspace publish".to_string()),
            "## Summary\n\n- Added publish descriptions".to_string(),
        ),
    )
    .await
    .unwrap();

    let saved = repo
        .get_pr_description(&conversation_id)
        .await
        .unwrap()
        .expect("description should be saved");
    assert_eq!(
        saved.title.as_deref(),
        Some("Describe agent workspace publish")
    );
    assert!(saved.body_markdown.contains("## Summary"));

    repo.clear_pr_description(&conversation_id).await.unwrap();
    assert!(repo
        .get_pr_description(&conversation_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn publication_events_round_trip_in_created_order() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();

    repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "checking",
        "started",
        "Checking workspace",
        None,
    ))
    .await
    .unwrap();
    repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "needs_agent",
        "failed",
        "Pre-commit hook failed",
        Some("agent_fixable".to_string()),
    ))
    .await
    .unwrap();

    let events = repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].step, "checking");
    assert_eq!(events[0].summary, "Checking workspace");
    assert_eq!(events[1].classification.as_deref(), Some("agent_fixable"));
}

#[tokio::test]
async fn list_active_direct_published_workspaces_filters_to_open_edit_workspaces() {
    let (db, repo, conversation_id) = setup_repo();
    let mut published = make_workspace(conversation_id);
    published.publication_pr_number = Some(72);
    published.publication_pr_url = Some("https://github.com/owner/repo/pull/72".to_string());
    published.publication_pr_status = Some("open".to_string());
    repo.create_or_update(published.clone()).await.unwrap();

    let archived_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &archived_id);
    let mut archived = make_workspace(archived_id);
    archived.status = AgentConversationWorkspaceStatus::Archived;
    archived.publication_pr_number = Some(73);
    repo.create_or_update(archived).await.unwrap();

    let execution_owned_id =
        ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    seed_conversation(&db, &execution_owned_id);
    let mut execution_owned = make_workspace(execution_owned_id);
    execution_owned.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
    execution_owned.publication_pr_number = Some(74);
    repo.create_or_update(execution_owned).await.unwrap();

    let ideation_id = ChatConversationId::from_string("77777777-7777-7777-7777-777777777777");
    seed_conversation(&db, &ideation_id);
    let mut ideation = make_workspace(ideation_id);
    ideation.mode = AgentConversationWorkspaceMode::Ideation;
    ideation.publication_pr_number = Some(77);
    ideation.publication_pr_status = Some("open".to_string());
    ideation.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(ideation).await.unwrap();

    let closed_id = ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
    seed_conversation(&db, &closed_id);
    let mut closed = make_workspace(closed_id);
    closed.publication_pr_number = Some(75);
    closed.publication_pr_status = Some("closed".to_string());
    repo.create_or_update(closed).await.unwrap();

    let needs_agent_id = ChatConversationId::from_string("55555555-5555-5555-5555-555555555555");
    seed_conversation(&db, &needs_agent_id);
    let mut needs_agent = make_workspace(needs_agent_id);
    needs_agent.publication_pr_number = Some(76);
    needs_agent.publication_pr_status = Some("changes_requested".to_string());
    needs_agent.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(needs_agent).await.unwrap();

    let workspaces = repo
        .list_active_direct_published_workspaces()
        .await
        .unwrap();

    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].conversation_id, published.conversation_id);
    assert_eq!(workspaces[0].publication_pr_number, Some(72));
}

#[tokio::test]
async fn list_active_needs_agent_workspaces_filters_to_open_active_workspaces() {
    let (db, repo, conversation_id) = setup_repo();
    let mut needs_agent = make_workspace(conversation_id);
    needs_agent.publication_pr_number = Some(82);
    needs_agent.publication_pr_status = Some("failed".to_string());
    needs_agent.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(needs_agent.clone()).await.unwrap();

    let closed_id = ChatConversationId::from_string("88888888-8888-8888-8888-888888888888");
    seed_conversation(&db, &closed_id);
    let mut closed = make_workspace(closed_id);
    closed.publication_pr_number = Some(83);
    closed.publication_pr_status = Some("closed".to_string());
    closed.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(closed).await.unwrap();

    let archived_id = ChatConversationId::from_string("99999999-9999-9999-9999-999999999999");
    seed_conversation(&db, &archived_id);
    let mut archived = make_workspace(archived_id);
    archived.status = AgentConversationWorkspaceStatus::Archived;
    archived.publication_pr_number = Some(84);
    archived.publication_pr_status = Some("failed".to_string());
    archived.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(archived).await.unwrap();

    let pushed_id = ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    seed_conversation(&db, &pushed_id);
    let mut pushed = make_workspace(pushed_id);
    pushed.publication_pr_number = Some(85);
    pushed.publication_pr_status = Some("open".to_string());
    pushed.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(pushed).await.unwrap();

    let workspaces = repo.list_active_needs_agent_workspaces().await.unwrap();

    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].conversation_id, needs_agent.conversation_id);
    assert_eq!(
        workspaces[0].publication_push_status.as_deref(),
        Some("needs_agent")
    );
}
