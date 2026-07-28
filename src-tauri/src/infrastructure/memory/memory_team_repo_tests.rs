use crate::domain::entities::{TeamSessionId, TeamSessionStatus};
use crate::domain::repositories::TeamRepository;
use crate::infrastructure::memory::MemoryTeamRepository;
use crate::testing::team_fixtures::{team_conversation_id, team_member, team_session};

#[tokio::test]
async fn test_ensure_session_returns_existing_open_session() {
    let repo = MemoryTeamRepository::new();

    repo.ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    let ensured = repo
        .ensure_session(team_session("team-2", &team_conversation_id(1)))
        .await
        .unwrap();
    assert_eq!(ensured.id.as_str(), "team-1");

    let open = repo
        .get_open_session_for_conversation(&team_conversation_id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(open.id.as_str(), "team-1");
}

#[tokio::test]
async fn test_ensure_session_after_close_creates_new_session() {
    let repo = MemoryTeamRepository::new();

    let mut session = repo
        .ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    session.status = TeamSessionStatus::Closed;
    session.version += 1;
    assert!(repo.update_session(session, 0).await.unwrap());

    let replacement = repo
        .ensure_session(team_session("team-2", &team_conversation_id(1)))
        .await
        .unwrap();
    assert_eq!(replacement.id.as_str(), "team-2");
}

#[tokio::test]
async fn test_update_session_rejects_stale_version() {
    let repo = MemoryTeamRepository::new();

    let mut session = repo
        .ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    session.version = 1;
    assert!(repo.update_session(session.clone(), 0).await.unwrap());
    assert!(!repo.update_session(session, 0).await.unwrap());
}

#[tokio::test]
async fn test_member_names_unique_per_team_and_generation_cas() {
    let repo = MemoryTeamRepository::new();

    repo.create_member(team_member("member-1", "team-1", "researcher"))
        .await
        .unwrap();
    assert!(repo
        .create_member(team_member("member-2", "team-1", "researcher"))
        .await
        .is_err());
    repo.create_member(team_member("member-3", "team-2", "researcher"))
        .await
        .unwrap();

    let mut updated = team_member("member-1", "team-1", "researcher");
    updated.generation = 1;
    assert!(repo.update_member(updated.clone(), 0).await.unwrap());
    assert!(!repo.update_member(updated, 0).await.unwrap());

    let members = repo
        .list_members(&TeamSessionId::from_string("team-1"))
        .await
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].generation, 1);
}
