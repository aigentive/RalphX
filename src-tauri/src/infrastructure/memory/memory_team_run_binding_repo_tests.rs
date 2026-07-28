use crate::domain::entities::{TeamMemberId, TeamRunBindingId, TeamRunBindingStatus};
use crate::domain::repositories::TeamRunBindingRepository;
use crate::infrastructure::memory::MemoryTeamRunBindingRepository;
use crate::testing::team_fixtures::{fixed_time, member_run_binding, team_run_binding};

#[tokio::test]
async fn test_agent_run_id_is_unique() {
    let repo = MemoryTeamRunBindingRepository::new();

    repo.create(team_run_binding("binding-1", "team-1", 1))
        .await
        .unwrap();
    assert!(repo
        .create(team_run_binding("binding-2", "team-1", 1))
        .await
        .is_err());
}

#[tokio::test]
async fn test_member_null_binding_must_be_coordination_only() {
    let repo = MemoryTeamRunBindingRepository::new();

    let mut invalid = team_run_binding("binding-1", "team-1", 1);
    invalid.team_member_generation = Some(1);
    assert!(repo.create(invalid).await.is_err());
}

#[tokio::test]
async fn test_current_member_binding_and_transition_cas() {
    let repo = MemoryTeamRunBindingRepository::new();
    let id = TeamRunBindingId::from_string("binding-1");

    let mut binding = repo
        .create(member_run_binding("binding-1", "team-1", 1, "member-1", 2))
        .await
        .unwrap();

    let current = repo
        .get_current_member_binding(&TeamMemberId::from_string("member-1"), 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.id.as_str(), "binding-1");
    assert!(repo
        .get_current_member_binding(&TeamMemberId::from_string("member-1"), 3)
        .await
        .unwrap()
        .is_none());

    binding
        .transition_to(TeamRunBindingStatus::Launching, fixed_time())
        .unwrap();
    binding.version = 1;
    assert!(repo.transition(&id, 0, binding.clone()).await.unwrap());
    assert!(!repo.transition(&id, 0, binding).await.unwrap());
}
