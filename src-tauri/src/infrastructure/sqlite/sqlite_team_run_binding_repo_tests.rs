use crate::domain::entities::{
    TeamMemberId, TeamRunBindingId, TeamRunBindingStatus, TeamSessionId,
};
use crate::domain::repositories::TeamRunBindingRepository;
use crate::infrastructure::sqlite::SqliteTeamRunBindingRepository;
use crate::testing::team_fixtures::{
    fixed_time, member_run_binding, seed_team_member_row, seed_team_session_row, team_agent_run_id,
    team_run_binding,
};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteTeamRunBindingRepository) {
    let db = SqliteTestDb::new("sqlite-team-run-binding-repo");
    db.with_connection(|conn| {
        seed_team_session_row(conn, "team-1", 101);
        seed_team_session_row(conn, "team-2", 102);
        seed_team_member_row(conn, "member-1", "team-1");
    });
    let repo = SqliteTeamRunBindingRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn test_create_and_roundtrip_binding() {
    let (_db, repo) = setup_repo();

    let created = repo
        .create(member_run_binding("binding-1", "team-1", 1, "member-1", 3))
        .await
        .unwrap();
    let fetched = repo
        .get_by_id(&TeamRunBindingId::from_string("binding-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.agent_run_id, team_agent_run_id(1));
    assert_eq!(fetched.team_member_generation, Some(3));
    assert_eq!(fetched.status, created.status);
    assert_eq!(fetched.created_at, created.created_at);

    let by_run = repo
        .get_by_agent_run_id(&team_agent_run_id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_run.id.as_str(), "binding-1");
}

#[tokio::test]
async fn test_agent_run_id_is_unique() {
    let (_db, repo) = setup_repo();

    repo.create(team_run_binding("binding-1", "team-1", 1))
        .await
        .unwrap();
    assert!(
        repo.create(team_run_binding("binding-2", "team-1", 1))
            .await
            .is_err(),
        "one agent run must bind to at most one team run binding"
    );
}

#[tokio::test]
async fn test_member_null_binding_must_be_coordination_only() {
    let (_db, repo) = setup_repo();

    let mut invalid = team_run_binding("binding-1", "team-1", 1);
    invalid.team_member_generation = Some(1);
    assert!(repo.create(invalid).await.is_err());
}

#[tokio::test]
async fn test_get_current_member_binding_scopes_by_generation() {
    let (_db, repo) = setup_repo();
    let member_id = TeamMemberId::from_string("member-1");

    repo.create(member_run_binding(
        "binding-old",
        "team-1",
        1,
        "member-1",
        1,
    ))
    .await
    .unwrap();
    repo.create(member_run_binding(
        "binding-new",
        "team-1",
        2,
        "member-1",
        2,
    ))
    .await
    .unwrap();

    let current = repo
        .get_current_member_binding(&member_id, 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.id.as_str(), "binding-new");
    assert!(repo
        .get_current_member_binding(&member_id, 3)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_transition_cas_rejects_stale_version() {
    let (_db, repo) = setup_repo();
    let id = TeamRunBindingId::from_string("binding-1");

    let mut binding = repo
        .create(member_run_binding("binding-1", "team-1", 1, "member-1", 0))
        .await
        .unwrap();
    binding
        .transition_to(TeamRunBindingStatus::Launching, fixed_time())
        .unwrap();
    binding.version = 1;

    assert!(repo.transition(&id, 0, binding.clone()).await.unwrap());
    assert!(
        !repo.transition(&id, 0, binding).await.unwrap(),
        "stale version must not transition"
    );

    let stored = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(stored.status, TeamRunBindingStatus::Launching);
    assert!(stored.launched_at.is_some());
}

#[tokio::test]
async fn test_list_for_team_filters_other_teams() {
    let (_db, repo) = setup_repo();

    repo.create(team_run_binding("binding-1", "team-1", 1))
        .await
        .unwrap();
    repo.create(team_run_binding("binding-2", "team-2", 2))
        .await
        .unwrap();

    let bindings = repo
        .list_for_team(&TeamSessionId::from_string("team-1"))
        .await
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].id.as_str(), "binding-1");
}
