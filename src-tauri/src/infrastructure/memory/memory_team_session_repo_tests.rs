use super::*;

#[tokio::test]
async fn test_create_and_get() {
    let repo = MemoryTeamSessionRepository::new();
    let session = TeamSession::new("team-1", "ctx-1", "task");
    let id = session.id.clone();

    repo.create(session).await.unwrap();
    let found = repo.get_by_id(&id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().team_name, "team-1");
}

#[tokio::test]
async fn test_get_by_context() {
    let repo = MemoryTeamSessionRepository::new();
    let s1 = TeamSession::new("team-a", "ctx-1", "task");
    let s2 = TeamSession::new("team-b", "ctx-2", "project");

    repo.create(s1).await.unwrap();
    repo.create(s2).await.unwrap();

    let results = repo.get_by_context("task", "ctx-1").await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_get_active_for_context() {
    let repo = MemoryTeamSessionRepository::new();
    let s1 = TeamSession::new("team-a", "ctx-1", "task");
    let id1 = s1.id.clone();

    repo.create(s1).await.unwrap();
    let active = repo.get_active_for_context("task", "ctx-1").await.unwrap();
    assert!(active.is_some());

    repo.set_disbanded(&id1).await.unwrap();
    let active = repo.get_active_for_context("task", "ctx-1").await.unwrap();
    assert!(active.is_none());
}

#[tokio::test]
async fn test_update_phase() {
    let repo = MemoryTeamSessionRepository::new();
    let session = TeamSession::new("team-1", "ctx-1", "task");
    let id = session.id.clone();

    repo.create(session).await.unwrap();
    repo.update_phase(&id, "working").await.unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.phase, "working");
}

#[tokio::test]
async fn test_update_teammates() {
    let repo = MemoryTeamSessionRepository::new();
    let session = TeamSession::new("team-1", "ctx-1", "task");
    let id = session.id.clone();

    repo.create(session).await.unwrap();

    let teammates = vec![TeammateSnapshot {
        name: "worker-1".to_string(),
        color: "#ff6b35".to_string(),
        model: "sonnet".to_string(),
        role: "coder".to_string(),
        status: "active".to_string(),
        cost: crate::application::team_state_tracker::TeammateCost {
            input_tokens: 1000,
            output_tokens: 500,
            cache_creation_tokens: 200,
            cache_read_tokens: 100,
            estimated_usd: 0.05,
        },
        spawned_at: "2024-01-01T00:00:00Z".to_string(),
        last_activity_at: "2024-01-01T00:01:00Z".to_string(),
        conversation_id: None,
    }];
    repo.update_teammates(&id, &teammates).await.unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.teammates.len(), 1);
    assert_eq!(found.teammates[0].name, "worker-1");
}
