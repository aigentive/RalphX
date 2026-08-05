use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;

use super::{startup_background::dispatch_one_remote_conversation_lifecycle, AppState};
use crate::application::remote_conversation_lifecycle_intent::{
    ALREADY_ARCHIVED, CHILD_EXISTS, PARENT_NOT_FORKABLE, PR_CLOSURE_FORBIDDEN,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, RemoteConversationLifecycleKind,
    RemoteConversationLifecycleRequest, RemoteConversationLifecycleStatus,
};
use crate::domain::repositories::RemoteConversationLifecycleRequestRepository;
use crate::domain::services::github_service::GithubServiceTrait;
use crate::domain::services::RunningAgentKey;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryRemoteConversationLifecycleRequestRepository;
use crate::tests::mock_github_service::MockGithubService;

fn request(
    conversation_id: &ChatConversationId,
    kind: RemoteConversationLifecycleKind,
    allocated_conversation_id: Option<String>,
    close_pull_request: bool,
    status: RemoteConversationLifecycleStatus,
    claimed_at: Option<chrono::DateTime<Utc>>,
) -> RemoteConversationLifecycleRequest {
    let now = Utc::now();
    RemoteConversationLifecycleRequest {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        conversation_id: conversation_id.as_str(),
        close_pull_request,
        allocated_conversation_id,
        status,
        error_code: None,
        result: None,
        claimed_at,
        created_at: now,
        updated_at: now,
    }
}

async fn project_conversation(
    state: &AppState,
    mode: AgentConversationWorkspaceMode,
) -> (Project, ChatConversation) {
    let project = Project::new("Lifecycle".into(), "/tmp/ralphx-lifecycle".into());
    state.project_repo.create(project.clone()).await.unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(mode));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    (project, conversation)
}

async fn persist(state: &AppState, row: RemoteConversationLifecycleRequest) -> String {
    state
        .remote_conversation_lifecycle_request_repo
        .create_remote_conversation_lifecycle_request(row.clone())
        .await
        .unwrap();
    row.id
}

async fn settled(state: &AppState, id: &str) -> RemoteConversationLifecycleRequest {
    state
        .remote_conversation_lifecycle_request_repo
        .get(id)
        .await
        .unwrap()
        .unwrap()
}

#[derive(Default)]
struct FailCompleteLifecycleRepository {
    inner: MemoryRemoteConversationLifecycleRequestRepository,
}

#[async_trait]
impl RemoteConversationLifecycleRequestRepository for FailCompleteLifecycleRepository {
    async fn create_remote_conversation_lifecycle_request(
        &self,
        row: RemoteConversationLifecycleRequest,
    ) -> AppResult<RemoteConversationLifecycleRequest> {
        self.inner
            .create_remote_conversation_lifecycle_request(row)
            .await
    }

    async fn get(&self, id: &str) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        self.inner.get(id).await
    }

    async fn find_unsettled(
        &self,
        conversation_id: &str,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        self.inner.find_unsettled(conversation_id).await
    }

    async fn claim_pending(
        &self,
        at: chrono::DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        self.inner.claim_pending(at).await
    }

    async fn complete(
        &self,
        _id: &str,
        _result: serde_json::Value,
        _at: chrono::DateTime<Utc>,
    ) -> AppResult<()> {
        Err(AppError::Database(
            "injected lifecycle completion failure".into(),
        ))
    }

    async fn fail(&self, id: &str, code: &str, at: chrono::DateTime<Utc>) -> AppResult<()> {
        self.inner.fail(id, code, at).await
    }

    async fn fail_stale(
        &self,
        before: chrono::DateTime<Utc>,
        at: chrono::DateTime<Utc>,
    ) -> AppResult<u64> {
        self.inner.fail_stale(before, at).await
    }
}

#[tokio::test]
async fn archive_completes_once_and_reentry_does_not_repeat_cleanup() {
    let state = AppState::new_test();
    let (_, conversation) =
        project_conversation(&state, AgentConversationWorkspaceMode::Plan).await;
    let id = persist(
        &state,
        request(
            &conversation.id,
            RemoteConversationLifecycleKind::Archive,
            None,
            false,
            RemoteConversationLifecycleStatus::Pending,
            None,
        ),
    )
    .await;

    dispatch_one_remote_conversation_lifecycle(&state)
        .await
        .unwrap();
    let first = settled(&state, &id).await;
    assert_eq!(first.status, RemoteConversationLifecycleStatus::Completed);
    assert_eq!(first.result.unwrap()["archived"], true);
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap()
        .is_archived());

    dispatch_one_remote_conversation_lifecycle(&state)
        .await
        .unwrap();
    assert_eq!(
        settled(&state, &id).await.status,
        RemoteConversationLifecycleStatus::Completed
    );
}

#[tokio::test]
async fn post_claim_repository_error_propagates_and_leaves_starting_for_stale_sweep() {
    let mut state = AppState::new_test();
    state.remote_conversation_lifecycle_request_repo =
        Arc::new(FailCompleteLifecycleRepository::default());
    let (_, conversation) =
        project_conversation(&state, AgentConversationWorkspaceMode::Plan).await;
    let id = persist(
        &state,
        request(
            &conversation.id,
            RemoteConversationLifecycleKind::Archive,
            None,
            false,
            RemoteConversationLifecycleStatus::Pending,
            None,
        ),
    )
    .await;

    assert!(matches!(
        dispatch_one_remote_conversation_lifecycle(&state).await,
        Err(AppError::Database(_))
    ));
    assert_eq!(
        settled(&state, &id).await.status,
        RemoteConversationLifecycleStatus::Starting
    );
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap()
        .is_archived());
}

#[tokio::test]
async fn archive_seam_read_error_propagates_without_terminal_settlement() {
    let state = AppState::new_test();
    let (_, conversation) =
        project_conversation(&state, AgentConversationWorkspaceMode::Edit).await;
    let workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        crate::domain::entities::ProjectId::from_string("missing-project".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        None,
        "ralphx/missing-project".into(),
        "/tmp/ralphx-missing-project".into(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let id = persist(
        &state,
        request(
            &conversation.id,
            RemoteConversationLifecycleKind::Archive,
            None,
            false,
            RemoteConversationLifecycleStatus::Pending,
            None,
        ),
    )
    .await;

    assert!(matches!(
        dispatch_one_remote_conversation_lifecycle(&state).await,
        Err(AppError::Infrastructure(_))
    ));
    assert_eq!(
        settled(&state, &id).await.status,
        RemoteConversationLifecycleStatus::Starting
    );
    assert!(!state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap()
        .is_archived());
}

#[tokio::test]
async fn already_archived_request_settles_benign_instead_of_failed() {
    let state = AppState::new_test();
    let (_, conversation) =
        project_conversation(&state, AgentConversationWorkspaceMode::Plan).await;
    state
        .chat_conversation_repo
        .archive(&conversation.id)
        .await
        .unwrap();
    let id = persist(
        &state,
        request(
            &conversation.id,
            RemoteConversationLifecycleKind::Archive,
            None,
            false,
            RemoteConversationLifecycleStatus::Pending,
            None,
        ),
    )
    .await;

    dispatch_one_remote_conversation_lifecycle(&state)
        .await
        .unwrap();
    let row = settled(&state, &id).await;
    assert_eq!(row.status, RemoteConversationLifecycleStatus::Completed);
    assert_eq!(row.result.unwrap()["code"], ALREADY_ARCHIVED);
}

#[tokio::test]
async fn archive_reproves_pr_closure_authority_at_claim_time_without_closing_pr() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);
    let (project, conversation) =
        project_conversation(&state, AgentConversationWorkspaceMode::Edit).await;
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        None,
        "ralphx/test".into(),
        "/tmp/ralphx-lifecycle-worktree".into(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".into());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let id = persist(
        &state,
        request(
            &conversation.id,
            RemoteConversationLifecycleKind::Archive,
            None,
            true,
            RemoteConversationLifecycleStatus::Pending,
            None,
        ),
    )
    .await;
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    dispatch_one_remote_conversation_lifecycle(&state)
        .await
        .unwrap();
    let row = settled(&state, &id).await;
    assert_eq!(row.status, RemoteConversationLifecycleStatus::Failed);
    assert_eq!(row.error_code.as_deref(), Some(PR_CLOSURE_FORBIDDEN));
    assert_eq!(github.state().close_pr_calls, 0);
    assert!(!state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap()
        .is_archived());
}

#[tokio::test]
async fn fork_uses_preallocated_child_once_and_reentry_creates_nothing_else() {
    let state = AppState::new_test();
    let (project, parent) =
        project_conversation(&state, AgentConversationWorkspaceMode::Plan).await;
    let child_id = ChatConversationId::new();
    let id = persist(
        &state,
        request(
            &parent.id,
            RemoteConversationLifecycleKind::Fork,
            Some(child_id.as_str()),
            false,
            RemoteConversationLifecycleStatus::Pending,
            None,
        ),
    )
    .await;

    dispatch_one_remote_conversation_lifecycle(&state)
        .await
        .unwrap();
    let row = settled(&state, &id).await;
    assert_eq!(row.status, RemoteConversationLifecycleStatus::Completed);
    assert_eq!(row.result.unwrap()["conversationId"], child_id.as_str());
    let children = state
        .chat_conversation_repo
        .get_by_context_filtered(ChatContextType::Project, project.id.as_str(), true)
        .await
        .unwrap();
    assert_eq!(children.len(), 2);
    assert!(children
        .iter()
        .any(|conversation| conversation.id == child_id));

    dispatch_one_remote_conversation_lifecycle(&state)
        .await
        .unwrap();
    assert_eq!(
        state
            .chat_conversation_repo
            .get_by_context_filtered(ChatContextType::Project, project.id.as_str(), true)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn existing_preallocated_child_settles_benign_without_creating_another_child() {
    let state = AppState::new_test();
    let (project, parent) =
        project_conversation(&state, AgentConversationWorkspaceMode::Plan).await;
    let mut child = ChatConversation::new_project(project.id.clone());
    child.id = ChatConversationId::new();
    state
        .chat_conversation_repo
        .create(child.clone())
        .await
        .unwrap();
    let id = persist(
        &state,
        request(
            &parent.id,
            RemoteConversationLifecycleKind::Fork,
            Some(child.id.as_str()),
            false,
            RemoteConversationLifecycleStatus::Pending,
            None,
        ),
    )
    .await;

    dispatch_one_remote_conversation_lifecycle(&state)
        .await
        .unwrap();
    let row = settled(&state, &id).await;
    assert_eq!(row.status, RemoteConversationLifecycleStatus::Completed);
    assert_eq!(row.result.unwrap()["code"], CHILD_EXISTS);
    assert_eq!(
        state
            .chat_conversation_repo
            .get_by_context_filtered(ChatContextType::Project, project.id.as_str(), true)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn fork_refuses_running_parent_and_tasks_mode() {
    for mode in [
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Tasks,
    ] {
        let state = AppState::new_test();
        let (_, parent) = project_conversation(&state, mode).await;
        if mode == AgentConversationWorkspaceMode::Plan {
            state
                .running_agent_registry
                .register(
                    RunningAgentKey::new("project", parent.id.as_str()),
                    0,
                    parent.id.as_str(),
                    "run".into(),
                    None,
                    None,
                )
                .await;
        }
        let child_id = ChatConversationId::new();
        let id = persist(
            &state,
            request(
                &parent.id,
                RemoteConversationLifecycleKind::Fork,
                Some(child_id.as_str()),
                false,
                RemoteConversationLifecycleStatus::Pending,
                None,
            ),
        )
        .await;

        dispatch_one_remote_conversation_lifecycle(&state)
            .await
            .unwrap();
        let row = settled(&state, &id).await;
        assert_eq!(row.status, RemoteConversationLifecycleStatus::Failed);
        assert_eq!(row.error_code.as_deref(), Some(PARENT_NOT_FORKABLE));
        assert!(state
            .chat_conversation_repo
            .get_by_id(&child_id)
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn stale_archive_and_fork_requests_are_terminal_and_never_dispatched() {
    for kind in [
        RemoteConversationLifecycleKind::Archive,
        RemoteConversationLifecycleKind::Fork,
    ] {
        let state = AppState::new_test();
        let (project, parent) =
            project_conversation(&state, AgentConversationWorkspaceMode::Plan).await;
        let child_id = ChatConversationId::new();
        let id = persist(
            &state,
            request(
                &parent.id,
                kind,
                (kind == RemoteConversationLifecycleKind::Fork).then(|| child_id.as_str()),
                false,
                RemoteConversationLifecycleStatus::Starting,
                Some(Utc::now() - Duration::seconds(301)),
            ),
        )
        .await;
        state
            .remote_conversation_lifecycle_request_repo
            .fail_stale(Utc::now() - Duration::seconds(300), Utc::now())
            .await
            .unwrap();

        dispatch_one_remote_conversation_lifecycle(&state)
            .await
            .unwrap();
        assert_eq!(
            settled(&state, &id).await.status,
            RemoteConversationLifecycleStatus::FailedStale
        );
        assert!(!state
            .chat_conversation_repo
            .get_by_id(&parent.id)
            .await
            .unwrap()
            .unwrap()
            .is_archived());
        assert_eq!(
            state
                .chat_conversation_repo
                .get_by_context_filtered(ChatContextType::Project, project.id.as_str(), true)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
