use std::path::Path;
use std::sync::Arc;

use crate::application::agent_runtime_context::{
    compose_agent_runtime_context, linked_plan_snapshot_resolver_from_app_state,
    AgentRuntimeContextDeps, AgentRuntimeContextScope, LinkedPlanSnapshotResolver,
};
use crate::application::AppState;
use crate::domain::entities::ideation::PLAN_CONTRACT_V2;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactType,
    ChatContextType, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, Project,
};
use crate::infrastructure::memory::{MemoryAgentTaskRepository, MemoryDelegatedSessionRepository};

struct LinkedPlanFixture {
    state: AppState,
    conversation_id: ChatConversationId,
    workspace: AgentConversationWorkspace,
    session: IdeationSession,
    overview: Artifact,
    blueprint: Artifact,
}

async fn linked_plan_fixture() -> LinkedPlanFixture {
    let state = AppState::new_sqlite_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Runtime linked plan".to_string(),
            "/tmp/ralphx-runtime-linked-plan".to_string(),
        ))
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("conversation-runtime-linked-plan");
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Plan Overview",
            ArtifactType::Specification,
            "secret overview body",
            "planner",
        ))
        .await
        .expect("overview should persist");
    let blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Implementation Blueprint",
            ArtifactType::Specification,
            "secret blueprint body",
            "planner",
        ))
        .await
        .expect("blueprint should persist");
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(crate::domain::entities::IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation_id.as_str())
                .plan_artifact_id(overview.id.clone())
                .plan_blueprint_artifact_id(blueprint.id.clone())
                .plan_contract_version(PLAN_CONTRACT_V2)
                .build(),
        )
        .await
        .expect("planning session should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "ralphx/runtime-linked-plan".to_string(),
        "/tmp/ralphx-runtime-linked-plan".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());

    LinkedPlanFixture {
        state,
        conversation_id,
        workspace,
        session,
        overview,
        blueprint,
    }
}

fn deps_with_linked_plan_snapshot(state: &AppState) -> AgentRuntimeContextDeps {
    AgentRuntimeContextDeps::new(
        Arc::new(MemoryDelegatedSessionRepository::new()),
        Arc::new(MemoryAgentTaskRepository::new()),
    )
    .with_linked_plan_snapshot_resolver(linked_plan_snapshot_resolver_from_app_state(state.clone()))
}

#[tokio::test]
async fn current_linked_plan_identity_is_available_for_each_workspace_mode_without_plan_bodies() {
    let fixture = linked_plan_fixture().await;
    let deps = deps_with_linked_plan_snapshot(&fixture.state);
    let mut workspace = fixture.workspace.clone();

    for mode in [
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Tasks,
    ] {
        workspace.mode = mode;
        let rendered = compose_agent_runtime_context(
            &AgentRuntimeContextScope {
                conversation_id: &fixture.conversation_id,
                context_type: ChatContextType::Project,
                context_id: "project-runtime-linked-plan",
                project_id: Some(workspace.project_id.as_str()),
                workspace: Some(&workspace),
                working_directory: Path::new("/tmp/ralphx-runtime-linked-plan"),
                entity_status: None,
            },
            &deps,
        )
        .await
        .expect("linked plan identity should render");

        assert!(rendered.contains("<linked_plan "));
        assert!(rendered.contains(&format!("session_id=\"{}\"", fixture.session.id)));
        assert!(rendered.contains(&format!(
            "plan_target_id=\"plan_bundle:v2:{}:{}\"",
            fixture.overview.id, fixture.blueprint.id
        )));
        assert!(rendered.contains("as_of=\""));
        assert!(rendered.contains(&format!("artifact_id=\"{}\"", fixture.overview.id)));
        assert!(rendered.contains(&format!("artifact_id=\"{}\"", fixture.blueprint.id)));
        assert!(rendered.contains("version=\"1\" title=\"Plan Overview\""));
        assert!(rendered.contains("version=\"1\" title=\"Implementation Blueprint\""));
        assert!(rendered.contains("status=\"draft\""));
        assert!(rendered.contains("current linked plan-bundle members"));
        assert!(!rendered.contains("secret overview body"));
        assert!(!rendered.contains("secret blueprint body"));
    }
}

#[tokio::test]
async fn next_compose_resolves_a_new_plan_version_without_message_level_changes() {
    let fixture = linked_plan_fixture().await;
    let deps = deps_with_linked_plan_snapshot(&fixture.state);
    let scope = AgentRuntimeContextScope {
        conversation_id: &fixture.conversation_id,
        context_type: ChatContextType::Project,
        context_id: "project-runtime-linked-plan",
        project_id: Some(fixture.workspace.project_id.as_str()),
        workspace: Some(&fixture.workspace),
        working_directory: Path::new("/tmp/ralphx-runtime-linked-plan"),
        entity_status: None,
    };
    let first = compose_agent_runtime_context(&scope, &deps)
        .await
        .expect("initial identity should render");
    assert!(first.contains(&format!(
        "artifact_id=\"{}\" version=\"1\"",
        fixture.overview.id
    )));

    let mut revised = Artifact::new_inline(
        "Plan Overview revised <&",
        ArtifactType::Specification,
        "new secret overview body",
        "planner",
    );
    revised.metadata.version = 2;
    let revised = fixture
        .state
        .artifact_repo
        .create_with_previous_version(revised, fixture.overview.id.clone())
        .await
        .expect("revised overview should persist");

    let next = compose_agent_runtime_context(&scope, &deps)
        .await
        .expect("revised identity should render");
    assert!(next.contains(&format!("artifact_id=\"{}\" version=\"2\"", revised.id)));
    assert!(next.contains("title=\"Plan Overview revised &lt;&amp;\""));
    assert!(next.contains(&format!(
        "plan_target_id=\"plan_bundle:v2:{}:{}\"",
        revised.id, fixture.blueprint.id
    )));
    assert!(!next.contains("new secret overview body"));
}

#[tokio::test]
async fn linked_plan_status_tracks_exact_approval_and_session_acceptance() {
    let fixture = linked_plan_fixture().await;
    let deps = deps_with_linked_plan_snapshot(&fixture.state);
    let scope = AgentRuntimeContextScope {
        conversation_id: &fixture.conversation_id,
        context_type: ChatContextType::Project,
        context_id: "project-runtime-linked-plan",
        project_id: Some(fixture.workspace.project_id.as_str()),
        workspace: Some(&fixture.workspace),
        working_directory: Path::new("/tmp/ralphx-runtime-linked-plan"),
        entity_status: None,
    };

    let session_id = fixture.session.id.clone();
    let overview_id = fixture.overview.id.as_str().to_string();
    fixture
        .state
        .db
        .run_transaction(move |connection| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                connection,
                session_id,
                Some(&overview_id),
                crate::domain::repositories::PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .expect("current bundle should be approved");
    let approved = compose_agent_runtime_context(&scope, &deps)
        .await
        .expect("approved identity should render");
    assert!(approved.contains("<linked_plan "));
    assert!(approved.contains("status=\"approved\""));

    fixture
        .state
        .ideation_session_repo
        .update_status(
            &fixture.session.id,
            crate::domain::entities::IdeationSessionStatus::Accepted,
        )
        .await
        .expect("session should become accepted");
    let accepted = compose_agent_runtime_context(&scope, &deps)
        .await
        .expect("accepted identity should render");
    assert!(accepted.contains("status=\"accepted\""));
}

struct EmptyLinkedPlanSnapshotResolver;

#[async_trait::async_trait]
impl LinkedPlanSnapshotResolver for EmptyLinkedPlanSnapshotResolver {
    async fn resolve(
        &self,
        _workspace: &AgentConversationWorkspace,
    ) -> Result<Option<crate::application::agent_plan_context::LinkedWorkspacePlanSnapshot>, String>
    {
        Ok(None)
    }
}

#[tokio::test]
async fn linked_session_without_a_current_bundle_omits_linked_plan_state() {
    let fixture = linked_plan_fixture().await;
    let deps = AgentRuntimeContextDeps::new(
        Arc::new(MemoryDelegatedSessionRepository::new()),
        Arc::new(MemoryAgentTaskRepository::new()),
    )
    .with_linked_plan_snapshot_resolver(Arc::new(EmptyLinkedPlanSnapshotResolver));

    let rendered = compose_agent_runtime_context(
        &AgentRuntimeContextScope {
            conversation_id: &fixture.conversation_id,
            context_type: ChatContextType::Project,
            context_id: "project-runtime-linked-plan",
            project_id: Some(fixture.workspace.project_id.as_str()),
            workspace: Some(&fixture.workspace),
            working_directory: Path::new("/tmp/ralphx-runtime-linked-plan"),
            entity_status: None,
        },
        &deps,
    )
    .await
    .expect("workspace and cold branch state should still render");

    assert!(!rendered.contains("<linked_plan"));
}

struct BrokenLinkedPlanSnapshotResolver;

#[async_trait::async_trait]
impl LinkedPlanSnapshotResolver for BrokenLinkedPlanSnapshotResolver {
    async fn resolve(
        &self,
        _workspace: &AgentConversationWorkspace,
    ) -> Result<Option<crate::application::agent_plan_context::LinkedWorkspacePlanSnapshot>, String>
    {
        Err("stale linked planning session".to_string())
    }
}

#[tokio::test]
async fn broken_link_is_explicitly_unavailable_without_exposing_resolver_details() {
    let fixture = linked_plan_fixture().await;
    let deps = AgentRuntimeContextDeps::new(
        Arc::new(MemoryDelegatedSessionRepository::new()),
        Arc::new(MemoryAgentTaskRepository::new()),
    )
    .with_linked_plan_snapshot_resolver(Arc::new(BrokenLinkedPlanSnapshotResolver));

    let rendered = compose_agent_runtime_context(
        &AgentRuntimeContextScope {
            conversation_id: &fixture.conversation_id,
            context_type: ChatContextType::Project,
            context_id: "project-runtime-linked-plan",
            project_id: Some(fixture.workspace.project_id.as_str()),
            workspace: Some(&fixture.workspace),
            working_directory: Path::new("/tmp/ralphx-runtime-linked-plan"),
            entity_status: None,
        },
        &deps,
    )
    .await
    .expect("runtime state should render unavailable linked plan state");

    assert!(rendered.contains("<linked_plan state=\"unavailable\" reason=\"resolution_error\"/>"));
    assert!(!rendered.contains("stale linked planning session"));
}
