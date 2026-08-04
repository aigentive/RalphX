use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};

use crate::application::agent_runtime_context::{
    compose_agent_runtime_context, AgentRuntimeContextDeps, AgentRuntimeContextScope,
};
use crate::domain::entities::{
    AgentTaskAssignmentId, ChatContextType, ChatConversationId, DelegatedSessionId, ProjectId,
    TeamMember, TeamMemberId, TeamMemberStatus, TeamSession, TeamSessionId, TeamSessionStatus,
};
use crate::domain::repositories::{
    AgentTaskRepository, DelegatedSessionRepository, TeamRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentTaskRepository, MemoryDelegatedSessionRepository, MemoryTeamRepository,
};

fn session(conversation_id: ChatConversationId) -> TeamSession {
    let now = Utc::now();
    TeamSession {
        id: TeamSessionId::from_string("team-<&"),
        project_id: ProjectId::from_string("project-<&".to_string()),
        coordinator_conversation_id: conversation_id,
        status: TeamSessionStatus::Active,
        strategy: None,
        configured_concurrency: 2,
        effective_concurrency: 2,
        automatic_wake_limit: 5,
        budget_policy: None,
        pending_coordination_mode: None,
        pending_exit_action: None,
        version: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        closed_at: None,
    }
}

fn member(index: usize, status: TeamMemberStatus) -> TeamMember {
    let now = Utc::now();
    TeamMember {
        id: TeamMemberId::from_string(format!("member-{index}")),
        team_id: TeamSessionId::from_string("team-<&"),
        normalized_name: format!("member {index}"),
        name: format!("Member {index}"),
        canonical_agent_name: format!("ralphx-general-worker-{index}"),
        role_summary: format!("Role {index} <&"),
        harness: None,
        logical_model: Some("must-not-render".to_string()),
        logical_effort: None,
        delegated_session_id: Some(DelegatedSessionId::from_string(format!(
            "delegate-{index}-<&"
        ))),
        generation: 1,
        current_run_id: None,
        current_assignment_id: None,
        status,
        last_activity_at: None,
        last_error: Some("must-not-render".to_string()),
        created_at: now,
        updated_at: now,
        stopped_at: None,
    }
}

fn scope(conversation_id: &ChatConversationId) -> AgentRuntimeContextScope<'_> {
    AgentRuntimeContextScope {
        conversation_id,
        context_type: ChatContextType::Project,
        context_id: "project-<&",
        project_id: Some("project-<&"),
        workspace: None,
        working_directory: Path::new("/tmp/runtime-context-team"),
        entity_status: None,
    }
}

fn deps(team_repo: Arc<dyn TeamRepository>) -> AgentRuntimeContextDeps {
    AgentRuntimeContextDeps::new(
        Arc::new(MemoryDelegatedSessionRepository::new()) as Arc<dyn DelegatedSessionRepository>,
        Arc::new(MemoryAgentTaskRepository::new()) as Arc<dyn AgentTaskRepository>,
    )
    .with_team_repo(team_repo)
}

#[tokio::test]
async fn coordinator_team_state_escapes_caps_and_excludes_stopped_members() {
    let conversation_id = ChatConversationId::from_string("coordinator-<&");
    let team_repo = Arc::new(MemoryTeamRepository::new());
    let team = team_repo
        .ensure_session(session(conversation_id.clone()))
        .await
        .expect("team should persist");
    for index in 0..25 {
        let mut team_member = member(index, TeamMemberStatus::Working);
        if index == 0 {
            team_member.current_assignment_id = Some(AgentTaskAssignmentId::from_string(
                "assignment-<&".to_string(),
            ));
            team_member.last_activity_at = Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap());
        }
        team_repo
            .create_member(team_member)
            .await
            .expect("member should persist");
    }
    team_repo
        .create_member(member(99, TeamMemberStatus::Stopped))
        .await
        .expect("stopped member should persist");

    let rendered = compose_agent_runtime_context(&scope(&conversation_id), &deps(team_repo))
        .await
        .expect("team state should render");

    assert!(rendered.contains(
        "<team_state session_id=\"team-&lt;&amp;\" status=\"active\" concurrency=\"2\" as_of=\""
    ));
    assert!(rendered.contains("name=\"Member 0\""));
    assert!(rendered.contains("status=\"working\" role=\"Role 0 &lt;&amp;\""));
    assert!(rendered.contains(
        "current_assignment=\"assignment-&lt;&amp;\" last_activity=\"2023-11-14T22:13:20+00:00\""
    ));
    assert!(rendered.contains("current_assignment=\"\" last_activity=\"unknown\""));
    assert!(rendered.contains("resolved from trusted runtime context"));
    assert!(rendered.contains("team_send_message, team_assign, or team_stop_member"));
    assert_eq!(rendered.matches("<member ").count(), 20);
    assert!(!rendered.contains("Member 99"));
    assert!(!rendered.contains("must-not-render"));
    assert_eq!(team.id.as_str(), "team-<&");
}

#[tokio::test]
async fn coordinator_team_state_keeps_the_session_header_when_no_members_are_live() {
    let conversation_id = ChatConversationId::from_string("empty-coordinator");
    let team_repo = Arc::new(MemoryTeamRepository::new());
    team_repo
        .ensure_session(session(conversation_id.clone()))
        .await
        .expect("team should persist");

    let rendered = compose_agent_runtime_context(&scope(&conversation_id), &deps(team_repo))
        .await
        .expect("an empty live roster should still identify the team session");

    assert!(rendered.contains("<team_state session_id=\"team-&lt;&amp;\""));
    assert_eq!(rendered.matches("<member ").count(), 0);
}

#[tokio::test]
async fn team_state_is_omitted_for_non_coordinator_conversations() {
    let conversation_id = ChatConversationId::from_string("coordinator");
    let team_repo = Arc::new(MemoryTeamRepository::new());
    team_repo
        .ensure_session(session(conversation_id.clone()))
        .await
        .expect("team should persist");
    team_repo
        .create_member(member(1, TeamMemberStatus::Idle))
        .await
        .expect("member should persist");

    let non_coordinator_id = ChatConversationId::new();
    let rendered =
        compose_agent_runtime_context(&scope(&non_coordinator_id), &deps(team_repo)).await;

    assert_eq!(
        rendered, None,
        "unexpected runtime state for non-coordinator"
    );
}

struct FailingTeamRepository;

#[async_trait]
impl TeamRepository for FailingTeamRepository {
    async fn ensure_session(&self, _session: TeamSession) -> AppResult<TeamSession> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn get_session(&self, _id: &TeamSessionId) -> AppResult<Option<TeamSession>> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn get_open_session_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<TeamSession>> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn list_open_sessions(&self) -> AppResult<Vec<TeamSession>> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn update_session(
        &self,
        _session: TeamSession,
        _expected_version: i64,
    ) -> AppResult<bool> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn create_member(&self, _member: TeamMember) -> AppResult<TeamMember> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn get_member(&self, _id: &TeamMemberId) -> AppResult<Option<TeamMember>> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn list_members(&self, _team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }

    async fn update_member(
        &self,
        _member: TeamMember,
        _expected_generation: i64,
    ) -> AppResult<bool> {
        Err(AppError::Infrastructure("unavailable".to_string()))
    }
}

#[tokio::test]
async fn team_state_reports_repository_errors_as_unavailable() {
    let conversation_id = ChatConversationId::from_string("coordinator");

    let rendered = compose_agent_runtime_context(
        &scope(&conversation_id),
        &deps(Arc::new(FailingTeamRepository)),
    )
    .await
    .expect("repository failures should remain visible");

    assert!(rendered.contains("<team_state state=\"unavailable\" reason=\"repository_error\"/>"));
}
