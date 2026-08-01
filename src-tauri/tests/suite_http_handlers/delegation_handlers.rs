use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::{fs, os::unix::fs::PermissionsExt};

use async_trait::async_trait;
use axum::{extract::State, http::HeaderMap, Json};
use chrono::{DateTime, Utc};
use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{
    AgentHarnessKind, AgentProviderSettings, LogicalEffort, ManualRoleDefault, ManualServiceTier,
    RoutingRole,
};
use ralphx_lib::domain::entities::agent_run::PersonaRunAttribution;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunActionKind,
    AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, AgentTaskAssignmentId,
    AgentTaskAssignmentReservation, AgentTaskAssignmentSettlement,
    AgentTaskAssignmentTerminalStatus, AgentTaskAssignmentView, AgentTaskCreate, AgentTaskDetail,
    AgentTaskListId, AgentTaskListSummary, AgentTaskMutationResult, AgentTaskPatch, AgentTaskScope,
    AgentTaskState, AgentTaskSummary, ChatContextType, ChatConversation, ChatConversationId,
    DelegatedSession, DelegatedSessionId, IdeationAnalysisBaseRefKind, IdeationSession,
    InterruptedConversation, Persona, PersonaId, PersonaStatus, Project, ProjectId, SessionPurpose,
    UsageCapture,
};
use ralphx_lib::domain::repositories::{
    AgentRunRepository, AgentTaskListOptions, AgentTaskRepository, DelegatedSessionRepository,
};
use ralphx_lib::error::{AppError, AppResult};
use ralphx_lib::http_server::delegation::{DelegationHistoryEntry, DelegationJobSnapshot};
use ralphx_lib::http_server::handlers::{
    build_delegated_task_completed_payload, build_delegated_task_started_payload, cancel_delegate,
    complete_delegate_assignment, get_delegate_assignment, get_delegated_session_status,
    start_delegate, start_delegate_with_runtime_context, wait_delegate,
};
use ralphx_lib::http_server::native_delegation_launcher::{
    NativeDelegationLaunchParent, NativeDelegationLaunchRequest, NativeDelegationLauncher,
};
use ralphx_lib::http_server::types::{
    CompleteDelegateAssignmentRequest, DelegateCancelRequest, DelegateStartRequest,
    DelegateWaitRequest, DelegatedRunSummary, HttpServerState,
};
use tempfile::TempDir;
use tokio::sync::Mutex;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a repo-root parent")
        .to_path_buf()
}

fn codex_cli_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn prepend_fake_codex_to_path(fake_codex_path: &Path) -> crate::support::env::EnvVarGuard {
    crate::support::env::prepend_to_path(
        fake_codex_path
            .parent()
            .expect("fake codex script should have parent dir"),
    )
}

fn install_fake_codex_cli() -> (TempDir, PathBuf) {
    let tempdir = TempDir::new().expect("tempdir");
    let script_path = tempdir.path().join("codex");
    let script = r#"#!/bin/sh
if [ "$1" = "--help" ]; then
cat <<'EOF'
Codex CLI

Commands:
  exec        Run Codex non-interactively [aliases: e]
  mcp         Manage external MCP servers for Codex
  resume      Resume a previous interactive session

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --search
      --add-dir <DIR>
EOF
exit 0
fi

if [ "$1" = "--version" ]; then
echo "codex-cli 0.116.0"
exit 0
fi

if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
cat <<'EOF'
Run Codex non-interactively

Usage: codex exec [OPTIONS] [PROMPT] [COMMAND]

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --add-dir <DIR>
      --json
  -C, --cd <DIR>
      --skip-git-repo-check
EOF
exit 0
fi

if [ "$1" = "exec" ]; then
if [ -n "$RALPHX_TEST_CODEX_ARGS_PATH" ]; then
  printf '%s\n' "$@" > "$RALPHX_TEST_CODEX_ARGS_PATH"
fi
if [ -n "$RALPHX_TEST_CODEX_CWD_PATH" ]; then
  pwd -P >> "$RALPHX_TEST_CODEX_CWD_PATH"
fi
printf '%s\n' '{"type":"thread.started","thread_id":"delegation-thread-1"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"MOCK_COMPLETION"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"total_token_usage":{"input_tokens":11,"cached_input_tokens":2,"output_tokens":7},"last_token_usage":{"input_tokens":11,"cached_input_tokens":2,"output_tokens":7}}}'
exit 0
fi

echo "unsupported invocation" >&2
exit 2
"#;
    fs::write(&script_path, script).expect("write fake codex cli");
    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("chmod fake codex cli");
    (tempdir, script_path)
}

async fn wait_for_captured_cwds(capture_path: &Path, expected_count: usize) -> Vec<PathBuf> {
    let capture_path = ralphx_lib::utils::path_safety::validate_absolute_non_root_path(
        capture_path,
        "test delegated cwd capture",
    )
    .expect("validate delegated cwd capture path");
    for _ in 0..40 {
        if let Ok(contents) = fs::read_to_string(&capture_path) {
            let paths = contents.lines().map(PathBuf::from).collect::<Vec<_>>();
            if paths.len() >= expected_count {
                return paths;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "expected {expected_count} delegated cwd captures at {}",
        capture_path.display()
    );
}

struct RemoveWorkspaceOnRunningDelegatedSessionRepository {
    inner: Arc<dyn DelegatedSessionRepository>,
    workspace_path: PathBuf,
}

#[async_trait]
impl DelegatedSessionRepository for RemoveWorkspaceOnRunningDelegatedSessionRepository {
    async fn create(&self, session: DelegatedSession) -> AppResult<DelegatedSession> {
        self.inner.create(session).await
    }

    async fn get_by_id(&self, id: &DelegatedSessionId) -> AppResult<Option<DelegatedSession>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_parent_context(
        &self,
        parent_context_type: &str,
        parent_context_id: &str,
    ) -> AppResult<Vec<DelegatedSession>> {
        self.inner
            .get_by_parent_context(parent_context_type, parent_context_id)
            .await
    }

    async fn update_provider_session_id(
        &self,
        id: &DelegatedSessionId,
        provider_session_id: Option<String>,
    ) -> AppResult<()> {
        self.inner
            .update_provider_session_id(id, provider_session_id)
            .await
    }

    async fn update_status(
        &self,
        id: &DelegatedSessionId,
        status: &str,
        error: Option<String>,
        completed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        self.inner
            .update_status(id, status, error, completed_at)
            .await?;
        if status == "running" {
            let workspace_path = ralphx_lib::utils::path_safety::validate_absolute_non_root_path(
                &self.workspace_path,
                "test workspace removed before delegated spawn",
            )?;
            fs::remove_dir_all(workspace_path).map_err(|error| {
                AppError::Infrastructure(format!(
                    "failed to remove test workspace before delegated spawn: {error}"
                ))
            })?;
        }
        Ok(())
    }
}

struct FailBindingAgentTaskRepository {
    inner: Arc<dyn AgentTaskRepository>,
}

#[async_trait]
impl AgentTaskRepository for FailBindingAgentTaskRepository {
    async fn create_task(
        &self,
        scope: &AgentTaskScope,
        input: AgentTaskCreate,
    ) -> AppResult<AgentTaskMutationResult> {
        self.inner.create_task(scope, input).await
    }

    async fn get_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
    ) -> AppResult<Option<AgentTaskDetail>> {
        self.inner.get_task(scope, task_ref).await
    }

    async fn list_tasks(
        &self,
        scope: &AgentTaskScope,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        self.inner.list_tasks(scope, options).await
    }

    async fn list_task_lists(
        &self,
        scope: &AgentTaskScope,
    ) -> AppResult<Vec<AgentTaskListSummary>> {
        self.inner.list_task_lists(scope).await
    }

    async fn list_tasks_for_list(
        &self,
        scope: &AgentTaskScope,
        list_id: &AgentTaskListId,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        self.inner
            .list_tasks_for_list(scope, list_id, options)
            .await
    }

    async fn update_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        patch: AgentTaskPatch,
    ) -> AppResult<Option<AgentTaskMutationResult>> {
        self.inner.update_task(scope, task_ref, patch).await
    }

    async fn reserve_assignment(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        delegated_session_id: &DelegatedSessionId,
        caller_agent_run_id: &AgentRunId,
        delegate_agent_name: &str,
    ) -> AppResult<Option<AgentTaskAssignmentReservation>> {
        self.inner
            .reserve_assignment(
                scope,
                task_ref,
                delegated_session_id,
                caller_agent_run_id,
                delegate_agent_name,
            )
            .await
    }

    async fn bind_assignment_run(
        &self,
        _assignment_id: &AgentTaskAssignmentId,
        _delegated_session_id: &DelegatedSessionId,
        _delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        Err(AppError::Infrastructure(
            "injected assignment binding failure".to_string(),
        ))
    }

    async fn plan_assignment_run(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        self.inner
            .plan_assignment_run(assignment_id, delegated_session_id, delegated_agent_run_id)
            .await
    }

    async fn set_assignment_team_identity(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        team_id: &ralphx_lib::domain::entities::TeamSessionId,
        team_member_id: &ralphx_lib::domain::entities::TeamMemberId,
        team_member_generation: i64,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        self.inner
            .set_assignment_team_identity(
                assignment_id,
                delegated_session_id,
                team_id,
                team_member_id,
                team_member_generation,
            )
            .await
    }

    async fn get_unresolved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        self.inner
            .get_unresolved_assignment(delegated_session_id)
            .await
    }

    async fn request_assignment_completion(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        local_scope: &AgentTaskScope,
        completion_metadata: Option<serde_json::Value>,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        self.inner
            .request_assignment_completion(
                delegated_session_id,
                delegated_agent_run_id,
                local_scope,
                completion_metadata,
            )
            .await
    }

    async fn request_assignment_release(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        self.inner
            .request_assignment_release(delegated_session_id, delegated_agent_run_id, reason)
            .await
    }

    async fn settle_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
        terminal_status: AgentTaskAssignmentTerminalStatus,
        reason: Option<&str>,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
        self.inner
            .settle_assignment_for_run(delegated_agent_run_id, terminal_status, reason)
            .await
    }

    async fn get_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        self.inner
            .get_assignment_for_run(delegated_agent_run_id)
            .await
    }

    async fn fail_reserved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
        self.inner
            .fail_reserved_assignment(delegated_session_id, reason)
            .await
    }

    async fn list_unresolved_assignments(&self) -> AppResult<Vec<AgentTaskAssignmentView>> {
        self.inner.list_unresolved_assignments().await
    }
}

struct FailCancelAgentRunRepository {
    inner: Arc<dyn AgentRunRepository>,
}

#[async_trait]
impl AgentRunRepository for FailCancelAgentRunRepository {
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun> {
        self.inner.create(run).await
    }

    async fn get_by_id(&self, id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_ids(&self, ids: &[AgentRunId]) -> AppResult<Vec<AgentRun>> {
        self.inner.get_by_ids(ids).await
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        self.inner
            .get_latest_for_conversation(conversation_id)
            .await
    }

    async fn get_latest_completed_for_provider_session(
        &self,
        conversation_id: &ChatConversationId,
        harness: AgentHarnessKind,
        provider_session_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        self.inner
            .get_latest_completed_for_provider_session(
                conversation_id,
                harness,
                provider_session_id,
            )
            .await
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        self.inner
            .get_active_for_conversation(conversation_id)
            .await
    }

    async fn get_latest_action(
        &self,
        conversation_id: &ChatConversationId,
        action_kind: AgentRunActionKind,
        action_context_id: &str,
        action_target_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        self.inner
            .get_latest_action(
                conversation_id,
                action_kind,
                action_context_id,
                action_target_id,
            )
            .await
    }

    async fn get_active_action(
        &self,
        conversation_id: &ChatConversationId,
        action_kind: AgentRunActionKind,
        action_context_id: &str,
        action_target_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        self.inner
            .get_active_action(
                conversation_id,
                action_kind,
                action_context_id,
                action_target_id,
            )
            .await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        self.inner.get_by_conversation(conversation_id).await
    }

    async fn update_status(&self, id: &AgentRunId, status: AgentRunStatus) -> AppResult<()> {
        self.inner.update_status(id, status).await
    }

    async fn update_usage(&self, id: &AgentRunId, usage: &AgentRunUsage) -> AppResult<()> {
        self.inner.update_usage(id, usage).await
    }

    async fn replace_usage_capture(
        &self,
        id: &AgentRunId,
        capture: &UsageCapture,
    ) -> AppResult<()> {
        self.inner.replace_usage_capture(id, capture).await
    }

    async fn update_attribution(
        &self,
        id: &AgentRunId,
        attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        self.inner.update_attribution(id, attribution).await
    }

    async fn set_persona_attribution(
        &self,
        id: &AgentRunId,
        attribution: PersonaRunAttribution,
    ) -> AppResult<()> {
        self.inner.set_persona_attribution(id, attribution).await
    }

    async fn complete(&self, id: &AgentRunId) -> AppResult<()> {
        self.inner.complete(id).await
    }

    async fn complete_if_running(&self, id: &AgentRunId) -> AppResult<bool> {
        self.inner.complete_if_running(id).await
    }

    async fn complete_if_prune_cancelled(&self, id: &AgentRunId) -> AppResult<bool> {
        self.inner.complete_if_prune_cancelled(id).await
    }

    async fn fail(&self, id: &AgentRunId, error_message: &str) -> AppResult<()> {
        self.inner.fail(id, error_message).await
    }

    async fn cancel(&self, _id: &AgentRunId) -> AppResult<()> {
        Err(AppError::Infrastructure(
            "injected agent-run cancellation failure".to_string(),
        ))
    }

    async fn cancel_with_reason(&self, _id: &AgentRunId, _reason: &str) -> AppResult<()> {
        Err(AppError::Infrastructure(
            "injected agent-run cancellation failure".to_string(),
        ))
    }

    async fn delete(&self, id: &AgentRunId) -> AppResult<()> {
        self.inner.delete(id).await
    }

    async fn delete_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.inner.delete_by_conversation(conversation_id).await
    }

    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> AppResult<u32> {
        self.inner.count_by_status(conversation_id, status).await
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        self.inner.cancel_all_running().await
    }

    async fn cancel_running_started_before(&self, cutoff: DateTime<Utc>) -> AppResult<u32> {
        self.inner.cancel_running_started_before(cutoff).await
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        self.inner.get_interrupted_conversations().await
    }
}

async fn seed_bound_active_project_persona(
    state: &HttpServerState,
    project_id: &ProjectId,
    persona_id: &str,
    body: &str,
) {
    let now = chrono::Utc::now();
    let persona = Persona {
        id: PersonaId::from(persona_id),
        artifact_id: None,

        project_id: None,
        slug: persona_id.to_string(),
        name: "Delegation isolation persona".to_string(),
        description: "Must not reach delegated children".to_string(),
        content: body.to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: format!("{persona_id}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .app_state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed active persona");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.persona_id = Some(persona.id.to_string());
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("bind active persona to a same-project conversation");
}

fn build_state(app_state: Arc<AppState>) -> HttpServerState {
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

struct NestedAssignmentCaller {
    project: Project,
    parent_conversation: ChatConversation,
    delegated_session: DelegatedSession,
    delegated_conversation: Option<ChatConversation>,
    local_scope: AgentTaskScope,
}

async fn seed_nested_assignment_caller(
    state: &HttpServerState,
    with_active_conversation: bool,
) -> NestedAssignmentCaller {
    let project = state
        .app_state
        .project_repo
        .create(Project::new(
            "Nested assignment project".to_string(),
            repo_root().display().to_string(),
        ))
        .await
        .expect("create nested assignment project");
    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create nested assignment parent conversation");
    let delegated_session = state
        .app_state
        .delegated_session_repo
        .create(DelegatedSession::new(
            project.id.clone(),
            "project",
            project.id.as_str(),
            "ralphx-general-worker",
            AgentHarnessKind::Codex,
        ))
        .await
        .expect("create caller delegated session");
    let delegated_conversation = if with_active_conversation {
        let mut conversation = ChatConversation::new_delegation(delegated_session.id.clone());
        conversation.parent_conversation_id = Some(parent_conversation.id.as_str());
        Some(
            state
                .app_state
                .chat_conversation_repo
                .create(conversation)
                .await
                .expect("create active caller delegated conversation"),
        )
    } else {
        None
    };
    let local_scope = AgentTaskScope::new("delegation", delegated_session.id.as_str());
    for title in ["Nested assigned task", "Meaningful local sibling"] {
        state
            .app_state
            .agent_task_repo
            .create_task(
                &local_scope,
                AgentTaskCreate {
                    title: title.to_string(),
                    details: format!("Requirements for {title}"),
                    active_label: None,
                    owner_agent: Some("ralphx-general-worker".to_string()),
                    metadata: None,
                    blocked_by: Vec::new(),
                    blocks: Vec::new(),
                },
            )
            .await
            .expect("create caller-local task");
    }

    NestedAssignmentCaller {
        project,
        parent_conversation,
        delegated_session,
        delegated_conversation,
        local_scope,
    }
}

fn nested_assignment_start_request(caller: &NestedAssignmentCaller) -> DelegateStartRequest {
    DelegateStartRequest {
        caller_agent_name: Some("ralphx-general-worker".to_string()),
        caller_agent_profile: None,
        caller_context_type: Some("delegation".to_string()),
        caller_context_id: Some(caller.delegated_session.id.as_str().to_string()),
        parent_session_id: None,
        parent_turn_id: None,
        parent_message_id: None,
        parent_conversation_id: Some(caller.parent_conversation.id.as_str()),
        parent_tool_use_id: None,
        delegated_session_id: None,
        child_session_id: None,
        task_ref: Some("1".to_string()),
        agent_name: "ralphx-general-explorer".to_string(),
        prompt: "Inspect the exact nested assignment.".to_string(),
        title: None,
        inherit_context: true,
        harness: Some(AgentHarnessKind::Codex),
        model: None,
        logical_effort: None,
        approval_policy: None,
        sandbox_mode: None,
    }
}

async fn seed_codex_provider_default(app_state: &AppState, model: &str, effort: LogicalEffort) {
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    codex.model = Some(model.to_string());
    codex.effort = Some(effort);
    app_state
        .agent_provider_settings_repo
        .upsert(&codex)
        .await
        .expect("Codex provider default should persist");
}

#[test]
fn delegate_start_request_accepts_legacy_message_alias_for_prompt() {
    let parsed: DelegateStartRequest = serde_json::from_str(
        r#"{
            "agent_name": "ralphx:ralphx-ideation-specialist-backend",
            "message": "SESSION_ID: parent\nAnalyze intent alignment."
        }"#,
    )
    .expect("legacy verifier payload should deserialize");

    assert_eq!(
        parsed.prompt,
        "SESSION_ID: parent\nAnalyze intent alignment."
    );
}

async fn create_parent_session(state: &HttpServerState) -> IdeationSession {
    create_parent_session_in_working_directory(state, &repo_root()).await
}

fn routed_delegate_start_request(
    parent_session_id: &str,
    parent_conversation_id: &str,
) -> DelegateStartRequest {
    DelegateStartRequest {
        caller_agent_name: Some("ralphx-ideation".to_string()),
        caller_agent_profile: None,
        caller_context_type: Some("ideation".to_string()),
        caller_context_id: Some(parent_session_id.to_string()),
        parent_session_id: Some(parent_session_id.to_string()),
        parent_turn_id: None,
        parent_message_id: None,
        parent_conversation_id: Some(parent_conversation_id.to_string()),
        parent_tool_use_id: None,
        delegated_session_id: None,
        child_session_id: None,
        task_ref: None,
        agent_name: "ralphx-general-explorer".to_string(),
        prompt: "Review the current change.".to_string(),
        title: None,
        inherit_context: true,
        harness: Some(AgentHarnessKind::Codex),
        model: None,
        logical_effort: None,
        approval_policy: None,
        sandbox_mode: None,
    }
}

async fn create_parent_session_in_working_directory(
    state: &HttpServerState,
    working_directory: &std::path::Path,
) -> IdeationSession {
    let project = Project::new(
        "Delegation Test Project".to_string(),
        working_directory.display().to_string(),
    );
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::builder()
        .project_id(project_id)
        .title("Delegation Parent")
        .cross_project_checked(true)
        .build();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap()
}

async fn create_project_agent_workspace(
    app_state: &AppState,
    worktree_parent: &Path,
) -> (Project, ChatConversation, AgentConversationWorkspace) {
    create_project_agent_workspace_with_harness(app_state, worktree_parent, AgentHarnessKind::Codex)
        .await
}

async fn create_project_agent_workspace_with_harness(
    app_state: &AppState,
    worktree_parent: &Path,
    parent_harness: AgentHarnessKind,
) -> (Project, ChatConversation, AgentConversationWorkspace) {
    let mut project = Project::new(
        "Delegation Agent Workspace Project".to_string(),
        repo_root().display().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.display().to_string());
    let project = app_state.project_repo.create(project).await.unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.provider_harness = Some(parent_harness);
    let conversation = app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let worktree_path =
        resolve_agent_conversation_workspace_path(&project, &conversation.id).unwrap();
    let safe_worktree_path = ralphx_lib::utils::path_safety::validate_absolute_non_root_path(
        &worktree_path,
        "test agent workspace",
    )
    .unwrap();
    fs::create_dir_all(safe_worktree_path.join(".git")).expect("create fake workspace git marker");
    let workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/agent-workspace-delegation".to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    let workspace = app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    (project, conversation, workspace)
}

fn install_runtime_plugin_dir() -> (TempDir, PathBuf) {
    let tempdir = TempDir::new().expect("tempdir");
    let plugin_dir = tempdir.path().join("plugins/app");
    fs::create_dir_all(&plugin_dir).expect("create temp plugin dir");
    let source_plugin_dir = repo_root().join("plugins/app");

    for entry in fs::read_dir(&source_plugin_dir).expect("read source plugin dir") {
        let entry = entry.expect("source plugin entry");
        symlink_path(entry.path(), plugin_dir.join(entry.file_name()));
    }

    (tempdir, plugin_dir)
}

#[tokio::test]
async fn test_delegate_start_creates_delegated_session_and_completes_with_mock_client() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;
    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent.id.clone()))
        .await
        .unwrap();
    let parent_conversation_id = parent_conversation.id.as_str();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-42".to_string()),
            parent_message_id: Some("msg-99".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-parent-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Review the proposal set and summarize the main implementation risks."
                .to_string(),
            title: Some("Delegated Risk Review".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_type, "ideation");
    assert_eq!(start.parent_context_id, parent.id.as_str());
    assert_eq!(start.parent_turn_id.as_deref(), Some("turn-42"));
    assert_eq!(start.parent_message_id.as_deref(), Some("msg-99"));
    assert_eq!(start.parent_tool_use_id.as_deref(), Some("toolu-parent-1"));
    assert_eq!(start.agent_name, "ralphx-ideation-specialist-backend");
    assert_eq!(start.harness, "codex");
    assert_eq!(start.status, "running");
    assert_ne!(start.delegated_session_id, parent.id.as_str());
    assert_eq!(
        start.parent_conversation_id.as_deref(),
        Some(parent_conversation_id.as_str())
    );
    assert!(start.delegated_conversation_id.is_some());
    assert!(start.delegated_agent_run_id.is_some());
    assert_eq!(start.history.len(), 1);
    assert_eq!(start.history[0].status, "running");

    let delegated_id = DelegatedSessionId::from_string(start.delegated_session_id.clone());
    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&delegated_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.parent_context_type, "ideation");
    assert_eq!(delegated.parent_context_id, parent.id.as_str());
    assert_eq!(delegated.status, "running");

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: Some(start.job_id.clone()),
                    job_ids: None,
                    wait_timeout_ms: None,
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(true),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    assert_eq!(waited.job_id, start.job_id);
    assert_eq!(waited.status, "completed");
    assert_eq!(waited.content.as_deref(), Some("MOCK_COMPLETION"));
    assert!(waited.error.is_none());
    assert_eq!(waited.parent_turn_id.as_deref(), Some("turn-42"));
    assert_eq!(waited.parent_message_id.as_deref(), Some("msg-99"));
    assert_eq!(waited.parent_tool_use_id.as_deref(), Some("toolu-parent-1"));
    assert_eq!(
        waited
            .history
            .iter()
            .map(|entry| entry.status.as_str())
            .collect::<Vec<_>>(),
        vec!["running", "completed"]
    );
    let delegated_status = waited
        .delegated_status
        .expect("delegated status should be hydrated");
    assert_eq!(delegated_status.session.id, waited.delegated_session_id);
    assert_eq!(delegated_status.session.parent_context_type, "ideation");
    assert_eq!(
        delegated_status.session.parent_context_id,
        parent.id.as_str()
    );
    assert_eq!(delegated_status.session.status, "completed");
    assert_eq!(delegated_status.agent_state.estimated_status, "completed");
    assert_eq!(
        delegated_status.conversation_id.as_deref(),
        waited.delegated_conversation_id.as_deref()
    );
    let latest_run = delegated_status.latest_run.expect("latest delegated run");
    assert_eq!(
        Some(latest_run.agent_run_id.as_str()),
        waited.delegated_agent_run_id.as_deref()
    );
    assert_eq!(latest_run.status, "completed");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(latest_run.logical_model, None);
    let recent_messages = delegated_status
        .recent_messages
        .expect("delegated status should expose handoff messages when requested");
    assert_eq!(recent_messages.len(), 1);
    assert_eq!(recent_messages[0].content, "MOCK_COMPLETION");

    let delegated_after = state
        .app_state
        .delegated_session_repo
        .get_by_id(&delegated_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated_after.status, "completed");
    assert!(delegated_after.completed_at.is_some());

    assert_eq!(latest_run.input_tokens, Some(11));
    assert_eq!(latest_run.cache_read_tokens, Some(2));
    assert_eq!(latest_run.output_tokens, Some(7));
}

#[tokio::test]
async fn native_delegation_launcher_does_not_create_http_delegation_job_state() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let (project, parent_conversation, workspace) = create_project_agent_workspace_with_harness(
        state.app_state.as_ref(),
        worktree_parent.path(),
        AgentHarnessKind::Codex,
    )
    .await;

    assert_eq!(state.delegation_service.job_count_for_test().await, 0);
    let launch = NativeDelegationLauncher::new(&state)
        .launch(NativeDelegationLaunchRequest {
            caller_agent_name: "ralphx-general-worker".to_string(),
            caller_agent_profile: None,
            parent: NativeDelegationLaunchParent {
                context_type: ChatContextType::Project,
                context_id: project.id.as_str().to_string(),
                project_id: project.id.as_str().to_string(),
                working_directory: PathBuf::from(workspace.worktree_path),
                caller_conversation_id: Some(parent_conversation.id.as_str()),
                parent_conversation_id: Some(parent_conversation.id.as_str()),
                ideation_verification: false,
            },
            caller_agent_run_id: None,
            target_agent_name: "ralphx-general-explorer".to_string(),
            reusable_delegated_session: None,
            task_ref: None,
            preallocated_agent_run_id: None,
            prompt: "Inspect the project without creating a delegation job.".to_string(),
            title: Some("Direct launcher test".to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_tool_use_id: None,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        })
        .await
        .expect("direct application launcher should start");

    assert!(!launch.delegated_session_id.is_empty());
    assert!(!launch.delegated_conversation_id.is_empty());
    assert!(!launch.delegated_agent_run_id.is_empty());
    assert_eq!(state.delegation_service.job_count_for_test().await, 0);
}

#[tokio::test]
async fn delegate_start_child_command_excludes_bound_project_persona() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_args_path = fake_codex_dir.path().join("delegated-child-args.txt");
    let _captured_args_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_ARGS_PATH",
        captured_args_path.clone(),
    );
    let _persona_flag_guard =
        crate::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "true");
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;
    let persona_body = "DELEGATION PERSONA BODY MUST NOT REACH CHILD";
    seed_bound_active_project_persona(
        &state,
        &parent.project_id,
        "delegation-cross-contamination-persona",
        persona_body,
    )
    .await;

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-persona-isolation".to_string()),
            parent_message_id: Some("msg-persona-isolation".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-persona-isolation".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Inspect delegated child persona isolation.".to_string(),
            title: Some("Delegated persona isolation".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("delegated child should spawn")
    .0;

    for _ in 0..20 {
        let status = wait_delegate(
            State(state.clone()),
            Json(DelegateWaitRequest {
                job_id: Some(start.job_id.clone()),
                job_ids: None,
                wait_timeout_ms: None,
                include_delegated_status: Some(false),
                include_child_status: None,
                include_messages: Some(false),
                message_limit: None,
            }),
        )
        .await
        .expect("delegation status should load")
        .0;
        if status.status != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let captured_args = fs::read_to_string(&captured_args_path)
        .expect("delegated child fake CLI should capture the final command arguments");
    assert!(
        !captured_args.contains("<ralphx_agent_persona>"),
        "delegated child command must not include a persona block: {captured_args}"
    );
    assert!(
        !captured_args.contains(persona_body),
        "delegated child command must not include the bound persona body: {captured_args}"
    );
}

#[tokio::test]
async fn test_delegate_start_from_project_agent_workspace_without_parent_session() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_args_path = fake_codex_dir.path().join("delegated-child-args.txt");
    let _captured_args_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_ARGS_PATH",
        captured_args_path.clone(),
    );
    let captured_cwd_path = fake_codex_dir.path().join("delegated-child-cwd.txt");
    let _captured_cwd_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_CWD_PATH",
        captured_cwd_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let (project, parent_conversation, workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let parent_conversation_id = parent_conversation.id.as_str();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-project".to_string()),
            parent_message_id: Some("msg-project".to_string()),
            parent_conversation_id: Some(parent_conversation_id.clone()),
            parent_tool_use_id: Some("toolu-project-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect <assigned> work & summarize the requested evidence.".to_string(),
            title: Some("Delegated Project Workspace Inspection".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_type, "project");
    assert_eq!(start.parent_context_id, project.id.as_str());
    assert_eq!(
        start.parent_conversation_id.as_deref(),
        Some(parent_conversation_id.as_str())
    );
    assert_eq!(start.harness, "codex");

    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(
            start.delegated_session_id.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.parent_context_type, "project");
    assert_eq!(delegated.parent_context_id, project.id.as_str());
    assert_eq!(delegated.project_id, project.id);
    assert_eq!(delegated.harness, AgentHarnessKind::Codex);

    let captured_cwds = wait_for_captured_cwds(&captured_cwd_path, 1).await;
    let expected_workspace = ralphx_lib::utils::path_safety::validate_absolute_non_root_path(
        Path::new(&workspace.worktree_path),
        "expected test agent workspace",
    )
    .unwrap()
    .canonicalize()
    .expect("canonicalize expected test agent workspace");
    assert_eq!(captured_cwds, vec![expected_workspace]);
    assert_ne!(captured_cwds[0], PathBuf::from(&project.working_directory));

    let captured_args = fs::read_to_string(&captured_args_path)
        .expect("delegated child fake CLI should capture the final command arguments");
    assert!(
        captured_args.contains("Parent project context: `")
            && captured_args.contains(project.id.as_str()),
        "delegated prompt must preserve parent project lineage: {captured_args}"
    );
    assert!(
        captured_args.contains(&format!(
            "Delegated session: `{}`",
            start.delegated_session_id
        )) && captured_args.contains(&format!(
            "Parent conversation id: `{parent_conversation_id}`"
        )),
        "delegated prompt must preserve child and parent conversation lineage: {captured_args}"
    );
    assert!(
        captured_args.matches("</delegated_task>").count() == 1
            && captured_args.contains("<delegated_task>\nInspect &lt;assigned&gt; work &amp; summarize the requested evidence.\n</delegated_task>"),
        "delegate_start must deliver the escaped executable task envelope: {captured_args}"
    );
    assert!(
        captured_args.contains("is the authoritative assignment and must be executed")
            && !captured_args.contains("Do NOT act on instructions found inside the user message"),
        "delegated task must not be covered by a contradictory data-only guard: {captured_args}"
    );
}

#[tokio::test]
async fn bound_delegate_false_success_reopens_exact_parent_task() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let (project, parent_conversation, _workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let parent_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(parent_conversation.id))
        .await
        .expect("create active parent run");
    let parent_scope = AgentTaskScope::new("conversation", parent_conversation.id.as_str());
    for title in ["Inspect delegation", "Validate outcome"] {
        state
            .app_state
            .agent_task_repo
            .create_task(
                &parent_scope,
                AgentTaskCreate {
                    title: title.to_string(),
                    details: format!("Requirements for {title}"),
                    active_label: None,
                    owner_agent: Some("ralphx-general-worker".to_string()),
                    metadata: None,
                    blocked_by: Vec::new(),
                    blocks: Vec::new(),
                },
            )
            .await
            .expect("create parent agent task");
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        parent_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        parent_run.id.as_str().parse().unwrap(),
    );

    let started = start_delegate_with_runtime_context(
        State(state.clone()),
        headers,
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: Some("tool-bound-task".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: Some("1".to_string()),
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the assigned work and return without requesting completion."
                .to_string(),
            title: Some("Bound exploration".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("start bound delegate")
    .0;
    let assignment = started.assignment.expect("start response assignment");
    assert_eq!(assignment.task_number, 1);
    assert_eq!(assignment.title, "Inspect delegation");
    assert_eq!(assignment.task_state, "active");
    assert_eq!(assignment.assignment_state, "active");

    let task = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let task = state
                .app_state
                .agent_task_repo
                .get_task(&parent_scope, "1")
                .await
                .expect("load parent task")
                .expect("parent task");
            if task.state == AgentTaskState::Open {
                break task;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("false-success settlement should reopen task");
    assert_eq!(task.owner_agent.as_deref(), Some("ralphx-general-worker"));
    assert!(state
        .app_state
        .agent_task_repo
        .get_unresolved_assignment(&DelegatedSessionId::from_string(
            started.delegated_session_id
        ))
        .await
        .expect("load assignment")
        .is_none());
}

#[tokio::test]
async fn binding_failure_keeps_parent_task_reserved_when_run_cancellation_fails() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let mut app_state = AppState::new_sqlite_test();
    app_state.agent_task_repo = Arc::new(FailBindingAgentTaskRepository {
        inner: Arc::clone(&app_state.agent_task_repo),
    });
    app_state.agent_run_repo = Arc::new(FailCancelAgentRunRepository {
        inner: Arc::clone(&app_state.agent_run_repo),
    });
    let state = build_state(Arc::new(app_state));
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let (project, parent_conversation, _workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let parent_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(parent_conversation.id))
        .await
        .expect("create active parent run");
    let parent_scope = AgentTaskScope::new("conversation", parent_conversation.id.as_str());
    for title in ["Inspect binding failure", "Keep meaningful ledger"] {
        state
            .app_state
            .agent_task_repo
            .create_task(
                &parent_scope,
                AgentTaskCreate {
                    title: title.to_string(),
                    details: format!("Requirements for {title}"),
                    active_label: None,
                    owner_agent: Some("ralphx-general-worker".to_string()),
                    metadata: None,
                    blocked_by: Vec::new(),
                    blocks: Vec::new(),
                },
            )
            .await
            .expect("create parent agent task");
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        parent_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        parent_run.id.as_str().parse().unwrap(),
    );

    let error = start_delegate_with_runtime_context(
        State(state.clone()),
        headers,
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: Some("tool-binding-failure".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: Some("1".to_string()),
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the assigned work.".to_string(),
            title: Some("Binding failure".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect_err("injected binding failure should reject delegate_start");
    assert_eq!(error.0, axum::http::StatusCode::INTERNAL_SERVER_ERROR);

    let task = state
        .app_state
        .agent_task_repo
        .get_task(&parent_scope, "1")
        .await
        .expect("load parent task")
        .expect("parent task");
    assert_eq!(task.state, AgentTaskState::Active);
    assert_eq!(task.owner_agent.as_deref(), Some("ralphx-general-explorer"));
    let delegated_session = state
        .app_state
        .delegated_session_repo
        .get_by_parent_context("project", project.id.as_str())
        .await
        .expect("load delegated sessions")
        .into_iter()
        .next()
        .expect("delegated session");
    let assignment = state
        .app_state
        .agent_task_repo
        .get_unresolved_assignment(&delegated_session.id)
        .await
        .expect("load unresolved assignment")
        .expect("reservation must remain unavailable");
    assert_eq!(assignment.assignment.state.as_str(), "reserved");
}

#[tokio::test]
async fn reused_unassigned_launch_does_not_bind_stale_reserved_attempt() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let (project, parent_conversation, _workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let parent_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(parent_conversation.id))
        .await
        .expect("create active parent run");
    let parent_scope = AgentTaskScope::new("conversation", parent_conversation.id.as_str());
    for title in ["Stale reserved work", "Keep meaningful ledger"] {
        state
            .app_state
            .agent_task_repo
            .create_task(
                &parent_scope,
                AgentTaskCreate {
                    title: title.to_string(),
                    details: format!("Requirements for {title}"),
                    active_label: None,
                    owner_agent: Some("ralphx-general-worker".to_string()),
                    metadata: None,
                    blocked_by: Vec::new(),
                    blocks: Vec::new(),
                },
            )
            .await
            .expect("create parent agent task");
    }

    let mut delegated_session = DelegatedSession::new(
        project.id.clone(),
        "project",
        project.id.as_str(),
        "ralphx-general-explorer",
        AgentHarnessKind::Codex,
    );
    delegated_session.status = "failed".to_string();
    let delegated_session = state
        .app_state
        .delegated_session_repo
        .create(delegated_session)
        .await
        .expect("create reusable delegated session");
    let mut delegated_conversation = ChatConversation::new_delegation(delegated_session.id.clone());
    delegated_conversation.parent_conversation_id = Some(parent_conversation.id.as_str());
    state
        .app_state
        .chat_conversation_repo
        .create(delegated_conversation)
        .await
        .expect("create delegated conversation");
    let reserved = state
        .app_state
        .agent_task_repo
        .reserve_assignment(
            &parent_scope,
            "1",
            &delegated_session.id,
            &parent_run.id,
            "ralphx-general-explorer",
        )
        .await
        .expect("reserve stale assignment")
        .expect("reserved assignment");

    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        parent_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        parent_run.id.as_str().parse().unwrap(),
    );
    let started = start_delegate_with_runtime_context(
        State(state.clone()),
        headers,
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: Some("tool-unassigned-retry".to_string()),
            delegated_session_id: Some(delegated_session.id.as_str().to_string()),
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Perform unrelated unassigned exploration.".to_string(),
            title: Some("Unassigned retry".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("unassigned reused launch should start")
    .0;

    assert!(
        started.assignment.is_none(),
        "an unassigned launch must not inherit an older reservation"
    );
    let unresolved = state
        .app_state
        .agent_task_repo
        .get_unresolved_assignment(&delegated_session.id)
        .await
        .expect("load unresolved assignment")
        .expect("stale reservation remains fenced for recovery");
    assert_eq!(unresolved.assignment.id, reserved.assignment.assignment.id);
    assert_eq!(
        unresolved.assignment.state.as_str(),
        "reserved",
        "the stale attempt must remain unbound"
    );
    assert!(unresolved.assignment.delegated_agent_run_id.is_none());
}

#[tokio::test]
async fn assignment_endpoints_require_exact_prebound_run_and_guard_unfinished_local_tasks() {
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let project = state
        .app_state
        .project_repo
        .create(Project::new(
            "Assignment endpoint project".to_string(),
            repo_root().display().to_string(),
        ))
        .await
        .unwrap();
    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let caller_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(parent_conversation.id))
        .await
        .unwrap();
    let delegated_session = state
        .app_state
        .delegated_session_repo
        .create(DelegatedSession::new(
            project.id,
            "project".to_string(),
            parent_conversation.context_id.clone(),
            "ralphx-general-worker".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .unwrap();
    state
        .app_state
        .delegated_session_repo
        .update_status(&delegated_session.id, "running", None, None)
        .await
        .unwrap();
    let delegated_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_delegation(
            delegated_session.id.clone(),
        ))
        .await
        .unwrap();
    let delegated_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(delegated_conversation.id))
        .await
        .unwrap();
    let parent_scope = AgentTaskScope::new("conversation", parent_conversation.id.as_str());
    let local_scope = AgentTaskScope::new("delegation", delegated_session.id.as_str());
    for (scope, titles) in [
        (&parent_scope, ["Assigned", "Sibling"]),
        (&local_scope, ["Local implementation", "Local validation"]),
    ] {
        for title in titles {
            state
                .app_state
                .agent_task_repo
                .create_task(
                    scope,
                    AgentTaskCreate {
                        title: title.to_string(),
                        details: format!("Requirements for {title}"),
                        active_label: None,
                        owner_agent: None,
                        metadata: None,
                        blocked_by: Vec::new(),
                        blocks: Vec::new(),
                    },
                )
                .await
                .unwrap();
        }
    }
    let reservation = state
        .app_state
        .agent_task_repo
        .reserve_assignment(
            &parent_scope,
            "1",
            &delegated_session.id,
            &caller_run.id,
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        delegated_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        delegated_run.id.as_str().parse().unwrap(),
    );

    let unbound = get_delegate_assignment(State(state.clone()), headers.clone())
        .await
        .0;
    assert!(unbound.success);
    assert!(unbound.assignment.is_none());
    let unresolved = state
        .app_state
        .agent_task_repo
        .get_unresolved_assignment(&delegated_session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unresolved.assignment.state.as_str(), "reserved");
    assert!(unresolved.assignment.delegated_agent_run_id.is_none());

    state
        .app_state
        .agent_task_repo
        .plan_assignment_run(
            &reservation.assignment.assignment.id,
            &delegated_session.id,
            &delegated_run.id,
        )
        .await
        .unwrap()
        .unwrap();
    state
        .app_state
        .agent_task_repo
        .bind_assignment_run(
            &reservation.assignment.assignment.id,
            &delegated_session.id,
            &delegated_run.id,
        )
        .await
        .unwrap()
        .unwrap();
    let inspected = get_delegate_assignment(State(state.clone()), headers.clone())
        .await
        .0;
    assert!(inspected.success);
    assert_eq!(inspected.assignment.unwrap().assignment_state, "active");
    let blocked = complete_delegate_assignment(
        State(state.clone()),
        headers.clone(),
        Json(CompleteDelegateAssignmentRequest { metadata: None }),
    )
    .await
    .0;
    assert!(!blocked.success);
    assert!(blocked
        .error
        .unwrap()
        .contains("delegate-local tasks must be resolved"));

    for task_ref in ["1", "2"] {
        state
            .app_state
            .agent_task_repo
            .update_task(
                &local_scope,
                task_ref,
                ralphx_lib::domain::entities::AgentTaskPatch {
                    state: Some(AgentTaskState::Done),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    let requested = complete_delegate_assignment(
        State(state),
        headers,
        Json(CompleteDelegateAssignmentRequest {
            metadata: Some(serde_json::json!({"verified": true})),
        }),
    )
    .await
    .0;
    assert!(requested.success);
    assert_eq!(
        requested.assignment.unwrap().assignment_state,
        "completion_requested"
    );
}

#[tokio::test]
async fn test_delegate_start_uses_delegated_subagent_provider_defaults() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_cwd_path = fake_codex_dir.path().join("provider-default-cwds.txt");
    let _captured_cwd_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_CWD_PATH",
        captured_cwd_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    seed_codex_provider_default(app_state.as_ref(), "gpt-5.6-terra", LogicalEffort::Medium).await;
    let state = build_state(app_state);
    let (project, parent_conversation, workspace) = create_project_agent_workspace_with_harness(
        state.app_state.as_ref(),
        worktree_parent.path(),
        AgentHarnessKind::Claude,
    )
    .await;

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-provider-default".to_string()),
            parent_message_id: Some("msg-provider-default".to_string()),
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the project using delegated defaults.".to_string(),
            title: Some("Delegated provider defaults".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("provider-default delegation should start")
    .0;

    assert_eq!(start.harness, "codex");
    assert_eq!(start.logical_model, None);
    assert_eq!(start.effective_model_id.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(start.effective_effort.as_deref(), Some("medium"));
    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(
            start.delegated_session_id.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.harness, AgentHarnessKind::Codex);

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: Some(start.job_id.clone()),
                    job_ids: None,
                    wait_timeout_ms: None,
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("provider-default delegation should settle")
    };
    let latest_run = waited
        .delegated_status
        .and_then(|status| status.latest_run)
        .expect("latest delegated run");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.logical_model, None);
    assert_eq!(
        latest_run.effective_model_id.as_deref(),
        Some("gpt-5.6-terra")
    );
    assert_eq!(latest_run.effective_effort.as_deref(), Some("medium"));
    let expected_workspace = ralphx_lib::utils::path_safety::validate_absolute_non_root_path(
        Path::new(&workspace.worktree_path),
        "expected provider-default agent workspace",
    )
    .unwrap()
    .canonicalize()
    .expect("canonicalize expected provider-default workspace");
    assert_eq!(
        wait_for_captured_cwds(&captured_cwd_path, 1).await,
        vec![expected_workspace.clone()]
    );
    assert_ne!(
        expected_workspace,
        PathBuf::from(&project.working_directory)
    );

    let partial = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-partial-default".to_string()),
            parent_message_id: Some("msg-partial-default".to_string()),
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the project with only a model override.".to_string(),
            title: Some("Delegated partial defaults".to_string()),
            inherit_context: true,
            harness: None,
            model: Some("gpt-5.6-explicit".to_string()),
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("partial-override delegation should start")
    .0;
    assert_eq!(partial.harness, "codex");
    assert_eq!(partial.logical_model.as_deref(), Some("gpt-5.6-explicit"));
    assert_eq!(
        partial.effective_model_id.as_deref(),
        Some("gpt-5.6-explicit")
    );
    assert_eq!(partial.effective_effort.as_deref(), Some("medium"));
    assert_eq!(
        wait_for_captured_cwds(&captured_cwd_path, 2).await,
        vec![expected_workspace.clone(), expected_workspace]
    );
}

#[tokio::test]
async fn test_delegate_start_rejects_reused_session_identity_conflicts() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_args_path = fake_codex_dir.path().join("reuse-conflict-args.txt");
    let _captured_args_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_ARGS_PATH",
        captured_args_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let (project, parent_conversation, _workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let mut existing = DelegatedSession::new(
        project.id.clone(),
        "project",
        project.id.as_str(),
        "ralphx-general-explorer",
        AgentHarnessKind::Codex,
    );
    existing.status = "pending".to_string();
    let existing = state
        .app_state
        .delegated_session_repo
        .create(existing)
        .await
        .expect("existing delegated session should persist");

    let agent_error = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: Some(existing.id.as_str().to_string()),
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-worker".to_string(),
            prompt: "This conflicting specialist must not launch.".to_string(),
            title: None,
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect_err("reused session specialist conflict should fail");
    assert!(agent_error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("agent")));

    let harness_error = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: Some(existing.id.as_str().to_string()),
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "This conflicting harness must not launch.".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Claude),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect_err("reused session harness conflict should fail");
    assert!(harness_error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("harness")));

    let stored = state
        .app_state
        .delegated_session_repo
        .get_by_id(&existing.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "pending");
    assert_eq!(stored.agent_name, "ralphx-general-explorer");
    assert_eq!(stored.harness, AgentHarnessKind::Codex);
    assert_eq!(stored.error, None);
    assert_eq!(stored.completed_at, None);
    assert!(state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Delegation, existing.id.as_str())
        .await
        .unwrap()
        .is_none());
    assert!(
        !captured_args_path.exists(),
        "no delegated process should spawn"
    );
    let sessions = state
        .app_state
        .delegated_session_repo
        .get_by_parent_context("project", project.id.as_str())
        .await
        .unwrap();
    assert_eq!(
        sessions.len(),
        1,
        "conflicts must not create another session"
    );

    let mut delegated_conversation = ChatConversation::new_delegation(existing.id.clone());
    delegated_conversation.parent_conversation_id = Some(parent_conversation.id.as_str());
    state
        .app_state
        .chat_conversation_repo
        .create(delegated_conversation)
        .await
        .expect("create matching delegated conversation lineage");
    let resumed = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: Some(existing.id.as_str().to_string()),
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Resume the matching delegated session.".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("matching caller lineage should permit session reuse")
    .0;
    assert_eq!(resumed.delegated_session_id, existing.id.as_str());
}

#[tokio::test]
async fn test_delegate_start_rejects_reused_session_from_another_project_conversation() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_args_path = fake_codex_dir
        .path()
        .join("cross-conversation-reuse-args.txt");
    let _captured_args_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_ARGS_PATH",
        captured_args_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let (project, first_parent_conversation, _workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let second_parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create second same-project parent conversation");
    let second_parent_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(second_parent_conversation.id))
        .await
        .expect("create active second parent run");

    let mut existing = DelegatedSession::new(
        project.id.clone(),
        "project",
        project.id.as_str(),
        "ralphx-general-explorer",
        AgentHarnessKind::Codex,
    );
    existing.status = "pending".to_string();
    let existing = state
        .app_state
        .delegated_session_repo
        .create(existing)
        .await
        .expect("existing delegated session should persist");
    let mut delegated_conversation = ChatConversation::new_delegation(existing.id.clone());
    delegated_conversation.parent_conversation_id = Some(first_parent_conversation.id.as_str());
    state
        .app_state
        .chat_conversation_repo
        .create(delegated_conversation)
        .await
        .expect("create delegated conversation under first parent");

    let second_parent_scope =
        AgentTaskScope::new("conversation", second_parent_conversation.id.as_str());
    for title in ["Assigned from second conversation", "Meaningful sibling"] {
        state
            .app_state
            .agent_task_repo
            .create_task(
                &second_parent_scope,
                AgentTaskCreate {
                    title: title.to_string(),
                    details: format!("Requirements for {title}"),
                    active_label: None,
                    owner_agent: Some("ralphx-general-worker".to_string()),
                    metadata: None,
                    blocked_by: Vec::new(),
                    blocks: Vec::new(),
                },
            )
            .await
            .expect("create second parent task");
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        second_parent_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        second_parent_run.id.as_str().parse().unwrap(),
    );

    let error = start_delegate_with_runtime_context(
        State(state.clone()),
        headers,
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(second_parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: Some(existing.id.as_str().to_string()),
            child_session_id: None,
            task_ref: Some("1".to_string()),
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "This cross-conversation reuse must not launch.".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect_err("same-project cross-conversation session reuse should fail");

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("lineage")));
    let task = state
        .app_state
        .agent_task_repo
        .get_task(&second_parent_scope, "1")
        .await
        .expect("load second parent task")
        .expect("second parent task");
    assert_eq!(task.state, AgentTaskState::Open);
    assert!(state
        .app_state
        .agent_task_repo
        .get_unresolved_assignment(&existing.id)
        .await
        .expect("load unresolved assignment")
        .is_none());
    assert!(
        !captured_args_path.exists(),
        "cross-conversation reuse must fail before process spawn"
    );
}

#[tokio::test]
async fn test_delegate_start_from_project_without_workspace_uses_project_checkout() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_cwd_path = fake_codex_dir.path().join("project-fallback-cwd.txt");
    let _captured_cwd_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_CWD_PATH",
        captured_cwd_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let project = state
        .app_state
        .project_repo
        .create(Project::new(
            "Delegation Project Fallback".to_string(),
            repo_root().display().to_string(),
        ))
        .await
        .unwrap();

    let _ = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the project checkout.".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("project fallback delegate should start");

    let captured_cwds = wait_for_captured_cwds(&captured_cwd_path, 1).await;
    assert_eq!(
        captured_cwds,
        vec![PathBuf::from(project.working_directory)]
    );
}

#[tokio::test]
async fn nested_assignment_rejects_missing_active_caller_conversation_before_reservation() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_args_path = fake_codex_dir
        .path()
        .join("missing-nested-caller-conversation-args.txt");
    let _captured_args_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_ARGS_PATH",
        captured_args_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let caller = seed_nested_assignment_caller(&state, false).await;
    let trusted_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(caller.parent_conversation.id))
        .await
        .expect("create unrelated trusted caller run");
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        caller.parent_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        trusted_run.id.as_str().parse().unwrap(),
    );

    let error = start_delegate_with_runtime_context(
        State(state.clone()),
        headers,
        Json(nested_assignment_start_request(&caller)),
    )
    .await
    .expect_err("nested assignment must require an active caller conversation");

    assert_eq!(error.0, axum::http::StatusCode::NOT_FOUND);
    assert!(error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("delegated conversation")));
    let task = state
        .app_state
        .agent_task_repo
        .get_task(&caller.local_scope, "1")
        .await
        .expect("load caller-local task")
        .expect("caller-local task");
    assert_eq!(task.state, AgentTaskState::Open);
    assert!(state
        .app_state
        .agent_task_repo
        .list_unresolved_assignments()
        .await
        .expect("list unresolved assignments")
        .is_empty());
    assert!(state
        .app_state
        .delegated_session_repo
        .get_by_parent_context("delegation", caller.delegated_session.id.as_str())
        .await
        .expect("list nested delegated sessions")
        .is_empty());
    assert!(
        !captured_args_path.exists(),
        "missing caller conversation must fail before process spawn"
    );
}

#[tokio::test]
async fn nested_assignment_rejects_mismatched_trusted_caller_conversation_before_reservation() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_args_path = fake_codex_dir
        .path()
        .join("mismatched-nested-caller-conversation-args.txt");
    let _captured_args_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_ARGS_PATH",
        captured_args_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let caller = seed_nested_assignment_caller(&state, true).await;
    let other_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(caller.project.id.clone()))
        .await
        .expect("create mismatched trusted conversation");
    let trusted_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(other_conversation.id))
        .await
        .expect("create mismatched trusted caller run");
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        other_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        trusted_run.id.as_str().parse().unwrap(),
    );

    let error = start_delegate_with_runtime_context(
        State(state.clone()),
        headers,
        Json(nested_assignment_start_request(&caller)),
    )
    .await
    .expect_err("nested assignment must reject a mismatched trusted caller conversation");

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("does not match")));
    assert!(caller.delegated_conversation.is_some());
    let task = state
        .app_state
        .agent_task_repo
        .get_task(&caller.local_scope, "1")
        .await
        .expect("load caller-local task")
        .expect("caller-local task");
    assert_eq!(task.state, AgentTaskState::Open);
    assert!(state
        .app_state
        .agent_task_repo
        .list_unresolved_assignments()
        .await
        .expect("list unresolved assignments")
        .is_empty());
    assert!(state
        .app_state
        .delegated_session_repo
        .get_by_parent_context("delegation", caller.delegated_session.id.as_str())
        .await
        .expect("list nested delegated sessions")
        .is_empty());
    assert!(
        !captured_args_path.exists(),
        "mismatched caller conversation must fail before process spawn"
    );
}

#[tokio::test]
async fn test_nested_delegate_preserves_original_project_agent_workspace() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_cwd_path = fake_codex_dir.path().join("nested-delegated-cwds.txt");
    let _captured_cwd_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_CWD_PATH",
        captured_cwd_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let (project, parent_conversation, workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let parent_conversation_id = parent_conversation.id.as_str();

    let first = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation_id.clone()),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-worker".to_string(),
            prompt: "Start the first workspace delegate.".to_string(),
            title: None,
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("first workspace delegate should start")
    .0;
    wait_for_captured_cwds(&captured_cwd_path, 1).await;
    let first_terminal = {
        let mut snapshot = None;
        for _ in 0..40 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: Some(first.job_id.clone()),
                    job_ids: None,
                    wait_timeout_ms: None,
                    include_delegated_status: Some(false),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .expect("first workspace delegate status should load")
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("first workspace delegate should settle before nested delegation")
    };
    assert_eq!(first_terminal.status, "completed");

    let delegated_conversation_id = first
        .delegated_conversation_id
        .clone()
        .expect("first delegate should expose its conversation");
    let nested_caller_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::from_string(
            delegated_conversation_id.clone(),
        )))
        .await
        .expect("create active outer delegate run");
    let mut nested_headers = HeaderMap::new();
    nested_headers.insert(
        "x-ralphx-agent-run-id",
        nested_caller_run.id.as_str().parse().unwrap(),
    );
    nested_headers.insert(
        "x-ralphx-conversation-id",
        delegated_conversation_id.parse().unwrap(),
    );

    let _ = start_delegate_with_runtime_context(
        State(state),
        nested_headers,
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("delegation".to_string()),
            caller_context_id: Some(first.delegated_session_id),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation_id),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the same workspace from the nested delegate.".to_string(),
            title: None,
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("nested workspace delegate should start");

    let captured_cwds = wait_for_captured_cwds(&captured_cwd_path, 2).await;
    let expected_workspace = ralphx_lib::utils::path_safety::validate_absolute_non_root_path(
        Path::new(&workspace.worktree_path),
        "expected nested test agent workspace",
    )
    .unwrap()
    .canonicalize()
    .expect("canonicalize expected nested test agent workspace");
    assert_eq!(
        captured_cwds,
        vec![expected_workspace.clone(), expected_workspace]
    );
    assert!(
        captured_cwds
            .iter()
            .all(|cwd| cwd != Path::new(&project.working_directory)),
        "nested delegation must never fall back to the project checkout"
    );
}

#[tokio::test]
async fn test_workspace_removed_before_delegate_spawn_marks_session_failed() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let captured_cwd_path = fake_codex_dir.path().join("missing-workspace-cwd.txt");
    let _captured_cwd_guard = crate::support::env::EnvVarGuard::set(
        "RALPHX_TEST_CODEX_CWD_PATH",
        captured_cwd_path.clone(),
    );
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let mut app_state = AppState::new_sqlite_test();
    let (project, parent_conversation, workspace) =
        create_project_agent_workspace(&app_state, worktree_parent.path()).await;
    let workspace_path = PathBuf::from(&workspace.worktree_path);
    let delegated_session_repo = Arc::clone(&app_state.delegated_session_repo);
    app_state.delegated_session_repo =
        Arc::new(RemoveWorkspaceOnRunningDelegatedSessionRepository {
            inner: delegated_session_repo,
            workspace_path,
        });
    let state = build_state(Arc::new(app_state));

    let error = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "This launch must fail closed.".to_string(),
            title: None,
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect_err("missing explicit workspace must fail the delegated launch");

    assert!(error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("Agent conversation workspace is missing")));
    assert!(
        !captured_cwd_path.exists(),
        "no delegated process should spawn"
    );
    let sessions = state
        .app_state
        .delegated_session_repo
        .get_by_parent_context("project", project.id.as_str())
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, "failed");
    assert!(sessions[0].completed_at.is_some());
    assert!(sessions[0]
        .error
        .as_deref()
        .is_some_and(|message| message.contains("Agent conversation workspace is missing")));
}

#[tokio::test]
async fn test_get_delegated_session_status_exposes_parent_context() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Publish a verification finding.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let status = get_delegated_session_status(
        State(state),
        axum::extract::Path(start.delegated_session_id.clone()),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(status.session.id, start.delegated_session_id);
    assert_eq!(status.session.parent_context_type, "ideation");
    assert_eq!(status.session.parent_context_id, parent.id.as_str());
}

#[tokio::test]
async fn test_delegate_start_does_not_invent_child_model_when_model_is_omitted() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: Some(start.job_id.clone()),
                    job_ids: None,
                    wait_timeout_ms: None,
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    let latest_run = waited
        .delegated_status
        .and_then(|status| status.latest_run)
        .expect("latest delegated run");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.logical_model, None);
    assert_eq!(latest_run.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        latest_run.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );
}

#[tokio::test]
async fn test_delegate_start_rejects_unknown_agent_name() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: Some("turn-bad".to_string()),
            parent_message_id: Some("msg-bad".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-does-not-exist".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("Unknown canonical agent"));
}

#[tokio::test]
async fn test_delegate_start_rejects_standalone_caller_context() {
    // D3.8 / Phase 4a.3: standalone (projectless) conversations are not a
    // delegation-capable caller context — the non-ideation caller branch in
    // resolve_delegate_parent must reject `caller_context_type: "standalone"`
    // before ever resolving a parent, agent identity, or harness. This is the
    // structural escape-hatch closure D9 relies on: standalone Chat mode has
    // no delegation rights.
    let state = build_state(Arc::new(AppState::new_sqlite_test()));

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-explorer".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("standalone".to_string()),
            caller_context_id: Some("standalone-conversation-id".to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("standalone"));
}

#[tokio::test]
async fn test_delegate_start_rejects_missing_caller_agent_name() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: None,
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("caller_agent_name"));
}

#[tokio::test]
async fn test_delegate_start_rejects_disallowed_target_for_caller() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-execution-worker".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::FORBIDDEN);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("may not delegate"));
}

#[tokio::test]
async fn test_delegate_start_enforces_profile_specific_allowed_targets() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: Some("plan".to_string()),
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::FORBIDDEN);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("may not delegate"));
}

/// `managed_team::members` passes `caller_agent_profile = "team_coordinator"` when a coordinator
/// launches a member. Before the profile-name validator accepted underscores, that profile failed
/// to resolve and this call returned `400 Unknown canonical caller agent` — the coordinator could
/// not delegate at all. Authorization must now come from the profile's own allowed targets.
#[tokio::test]
async fn test_delegate_start_enforces_team_coordinator_profile_allowed_targets() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-general-worker".to_string()),
            caller_agent_profile: Some("team_coordinator".to_string()),
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: Some(parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(
        error.0,
        axum::http::StatusCode::FORBIDDEN,
        "an out-of-allowlist target must fail authorization, not caller resolution: {:?}",
        error.1 .0
    );
    let message = error.1 .0["error"].as_str().unwrap_or_default().to_string();
    assert!(
        message.contains("may not delegate"),
        "expected an allowlist rejection, got: {message}"
    );
    assert!(
        !message.contains("Unknown canonical caller agent"),
        "the team_coordinator profile must resolve before authorization runs, got: {message}"
    );
}

/// Proves the positive half of the same obligation: both targets declared by the coordinator
/// profile's `delegation.allowed_targets` actually launch from an Edit-mode agent workspace.
#[tokio::test]
async fn test_delegate_start_allows_team_coordinator_profile_declared_targets() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let (project, parent_conversation, _workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let parent_conversation_id = parent_conversation.id.as_str();

    for target in ["ralphx-general-explorer", "ralphx-general-worker"] {
        let start = start_delegate(
            State(state.clone()),
            Json(DelegateStartRequest {
                caller_agent_name: Some("ralphx-general-worker".to_string()),
                caller_agent_profile: Some("team_coordinator".to_string()),
                caller_context_type: Some("project".to_string()),
                caller_context_id: Some(project.id.as_str().to_string()),
                parent_session_id: None,
                parent_turn_id: None,
                parent_message_id: None,
                parent_conversation_id: Some(parent_conversation_id.clone()),
                parent_tool_use_id: None,
                delegated_session_id: None,
                child_session_id: None,
                task_ref: None,
                agent_name: target.to_string(),
                prompt: "Inspect the assigned surface and report findings.".to_string(),
                title: Some(format!("Team member {target}")),
                inherit_context: true,
                harness: None,
                model: None,
                logical_effort: None,
                approval_policy: None,
                sandbox_mode: None,
            }),
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "team_coordinator must be allowed to delegate to {target}: {:?}",
                error.1 .0
            )
        })
        .0;

        assert_eq!(start.agent_name, target);
        assert_eq!(start.status, "running");
    }
}

#[tokio::test]
async fn test_delegate_start_enforces_workspace_reviewer_allowed_targets() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let worktree_parent = TempDir::new().expect("worktree parent");
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    seed_codex_provider_default(state.app_state.as_ref(), "gpt-5.5", LogicalEffort::XHigh).await;
    let (project, parent_conversation, _workspace) =
        create_project_agent_workspace(state.app_state.as_ref(), worktree_parent.path()).await;
    let parent_conversation_id = parent_conversation.id.as_str();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-workspace-reviewer".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation_id.clone()),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-explorer".to_string(),
            prompt: "Inspect the assigned surface and report findings.".to_string(),
            title: Some("Workspace review exploration".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .expect("workspace reviewer must be allowed to delegate to the general explorer")
    .0;

    assert_eq!(start.agent_name, "ralphx-general-explorer");
    assert_eq!(start.status, "running");

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-workspace-reviewer".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("project".to_string()),
            caller_context_id: Some(project.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation_id),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-general-worker".to_string(),
            prompt: "Inspect the assigned surface and report findings.".to_string(),
            title: Some("Workspace review implementation".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::FORBIDDEN);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("may not delegate"));
}

#[tokio::test]
async fn test_delegate_start_infers_parent_session_from_verification_child_context() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_id, parent.id.as_str());
}

#[tokio::test]
async fn test_delegate_start_verifier_context_survives_external_generated_plugin_dir() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let (_runtime_plugin_root, runtime_plugin_dir) = install_runtime_plugin_dir();
    let target_project_root = TempDir::new().expect("temp target project");
    let generated_plugin_root = TempDir::new().expect("temp generated plugin root");
    let generated_plugin_dir = generated_plugin_root.path().join("generated/claude-plugin");
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let _runtime_plugin_guard =
        ralphx_lib::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            runtime_plugin_dir,
            generated_plugin_dir.clone(),
        );
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent =
        create_parent_session_in_working_directory(&state, target_project_root.path()).await;

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(start.parent_context_id, parent.id.as_str());
    assert!(
        generated_plugin_dir.exists(),
        "materialized generated plugin dir should exist for external desktop-style layouts"
    );
}

#[tokio::test]
async fn test_legacy_verification_child_uses_ideation_subagent_harness_when_omitted() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    state
        .app_state
        .manual_role_default_repo
        .upsert_global(
            RoutingRole::IdeationVerifierSubagent,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-5.4-mini".to_string()),
                effort: None,
                service_tier: ManualServiceTier::Standard,
                coordination_mode: None,
                persona_id: None,
                approval_policy: Some("never".to_string()),
                sandbox_mode: Some("danger-full-access".to_string()),
            },
        )
        .await
        .expect("ideation verifier subagent role default should persist");

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: Some(start.job_id.clone()),
                    job_ids: None,
                    wait_timeout_ms: None,
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    let latest_run = waited
        .delegated_status
        .as_ref()
        .and_then(|status| status.latest_run.as_ref())
        .expect("latest delegated run");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        latest_run.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );

    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(
            start.delegated_session_id.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.harness, AgentHarnessKind::Codex);
}

#[cfg(unix)]
fn symlink_path(source: impl AsRef<std::path::Path>, target: impl AsRef<std::path::Path>) {
    std::os::unix::fs::symlink(source, target).expect("create symlink");
}

#[cfg(windows)]
fn symlink_path(source: impl AsRef<std::path::Path>, target: impl AsRef<std::path::Path>) {
    let source = source.as_ref();
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target).expect("create dir symlink");
    } else {
        std::os::windows::fs::symlink_file(source, target).expect("create file symlink");
    }
}

#[tokio::test]
async fn test_delegate_start_uses_ideation_subagent_harness_when_harness_is_omitted() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    state
        .app_state
        .manual_role_default_repo
        .upsert_global(
            RoutingRole::IdeationSubagent,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-5.4-mini".to_string()),
                effort: None,
                service_tier: ManualServiceTier::Standard,
                coordination_mode: None,
                persona_id: None,
                approval_policy: Some("never".to_string()),
                sandbox_mode: Some("danger-full-access".to_string()),
            },
        )
        .await
        .expect("ideation subagent role default should persist");

    let start = start_delegate(
        State(state.clone()),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(parent.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-ideation".to_string()),
            parent_message_id: Some("msg-ideation".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-ideation-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Analyze the plan intent and summarize any scope drift risks.".to_string(),
            title: Some("Delegated Intent Specialist".to_string()),
            inherit_context: true,
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    let waited = {
        let mut snapshot = None;
        for _ in 0..20 {
            let candidate = wait_delegate(
                State(state.clone()),
                Json(DelegateWaitRequest {
                    job_id: Some(start.job_id.clone()),
                    job_ids: None,
                    wait_timeout_ms: None,
                    include_delegated_status: Some(true),
                    include_child_status: None,
                    include_messages: Some(false),
                    message_limit: None,
                }),
            )
            .await
            .unwrap()
            .0;
            if candidate.status != "running" {
                snapshot = Some(candidate);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        snapshot.expect("delegation job should settle")
    };

    let latest_run = waited
        .delegated_status
        .as_ref()
        .and_then(|status| status.latest_run.as_ref())
        .expect("latest delegated run");
    assert_eq!(latest_run.harness.as_deref(), Some("codex"));
    assert_eq!(latest_run.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        latest_run.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );

    let delegated = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(
            start.delegated_session_id.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delegated.harness, AgentHarnessKind::Codex);
}

#[tokio::test]
async fn test_delegate_start_links_parent_conversation_to_verification_child_chat() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let app_state = Arc::new(AppState::new_sqlite_test());
    let state = build_state(app_state);
    let parent = create_parent_session(&state).await;

    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent.id.clone()))
        .await
        .unwrap();

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let verification_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(
            verification_child.id.clone(),
        ))
        .await
        .unwrap();
    let parent_conversation_id = parent_conversation.id.as_str();
    let verification_conversation_id = verification_conversation.id.as_str();

    let start = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: Some("turn-verifier".to_string()),
            parent_message_id: Some("msg-verifier".to_string()),
            parent_conversation_id: None,
            parent_tool_use_id: Some("toolu-verifier-1".to_string()),
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Review the plan for completeness and summarize any gaps.".to_string(),
            title: Some("Delegated Completeness Critic".to_string()),
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(
        start.parent_conversation_id.as_deref(),
        Some(verification_conversation_id.as_str())
    );
    assert_ne!(
        start.parent_conversation_id.as_deref(),
        Some(parent_conversation_id.as_str())
    );
}

#[tokio::test]
async fn verification_child_runtime_uses_current_run_authority_and_root_lineage() {
    let _env_lock = codex_cli_env_lock().lock().await;
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent.id.clone()))
        .await
        .unwrap();
    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();
    let verification_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(
            verification_child.id.clone(),
        ))
        .await
        .unwrap();
    let verification_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(verification_conversation.id))
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        verification_conversation.id.as_str().parse().unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        verification_run.id.as_str().parse().unwrap(),
    );

    let started = start_delegate_with_runtime_context(
        State(state),
        headers,
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: None,
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: Some(parent_conversation.id.as_str()),
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "Inspect the verified plan.".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(
        started.parent_conversation_id,
        Some(parent_conversation.id.as_str())
    );
    assert_eq!(
        started.parent_agent_run_id,
        Some(verification_run.id.as_str())
    );
}

#[tokio::test]
async fn test_delegate_start_rejects_parent_session_mismatch_against_verification_child_context() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let other_parent = create_parent_session(&state).await;

    let mut verification_child = IdeationSession::builder()
        .project_id(parent.project_id.clone())
        .title("Verification Child")
        .cross_project_checked(true)
        .build();
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    let verification_child = state
        .app_state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let error = start_delegate(
        State(state),
        Json(DelegateStartRequest {
            caller_agent_name: Some("ralphx-ideation".to_string()),
            caller_agent_profile: None,
            caller_context_type: Some("ideation".to_string()),
            caller_context_id: Some(verification_child.id.as_str().to_string()),
            parent_session_id: Some(other_parent.id.as_str().to_string()),
            parent_turn_id: None,
            parent_message_id: None,
            parent_conversation_id: None,
            parent_tool_use_id: None,
            delegated_session_id: None,
            child_session_id: None,
            task_ref: None,
            agent_name: "ralphx-ideation-specialist-backend".to_string(),
            prompt: "noop".to_string(),
            title: None,
            inherit_context: true,
            harness: Some(AgentHarnessKind::Codex),
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("does not match caller context parent"));
}

#[tokio::test]
async fn test_delegate_cancel_rejects_unknown_job() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));

    let error = cancel_delegate(
        State(state),
        Json(DelegateCancelRequest {
            job_id: "missing-job".to_string(),
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_routed_delegate_start_requires_trusted_parent_run_context() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));

    let error = start_delegate_with_runtime_context(
        State(state),
        HeaderMap::new(),
        Json(routed_delegate_start_request(
            "parent-session",
            "parent-conversation",
        )),
    )
    .await
    .unwrap_err();

    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(
        error.1 .0["error"].as_str().unwrap_or_default(),
        ralphx_lib::http_server::handlers::DELEGATION_MISSING_RUN_IDENTITY_ERROR,
    );
}

#[tokio::test]
async fn test_routed_delegate_start_rejects_wrong_conversation_and_stale_parent_runs() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let other_parent = create_parent_session(&state).await;
    let parent_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent.id.clone()))
        .await
        .expect("create parent conversation");
    let other_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(other_parent.id))
        .await
        .expect("create other conversation");

    let wrong_conversation_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(other_conversation.id))
        .await
        .expect("create wrong-conversation run");
    let mut wrong_headers = HeaderMap::new();
    wrong_headers.insert(
        "x-ralphx-agent-run-id",
        wrong_conversation_run.id.as_str().parse().unwrap(),
    );
    let wrong_error = start_delegate_with_runtime_context(
        State(state.clone()),
        wrong_headers,
        Json(routed_delegate_start_request(
            parent.id.as_str(),
            &parent_conversation.id.as_str(),
        )),
    )
    .await
    .unwrap_err();
    assert_eq!(wrong_error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(wrong_error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("does not belong to the caller conversation"));

    let mut stale_run = AgentRun::new(parent_conversation.id);
    stale_run.status = AgentRunStatus::Completed;
    stale_run.completed_at = Some(Utc::now());
    let stale_run = state
        .app_state
        .agent_run_repo
        .create(stale_run)
        .await
        .expect("create stale parent run");
    state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(parent_conversation.id))
        .await
        .expect("create active parent run");
    let mut stale_headers = HeaderMap::new();
    stale_headers.insert(
        "x-ralphx-agent-run-id",
        stale_run.id.as_str().parse().unwrap(),
    );
    let stale_error = start_delegate_with_runtime_context(
        State(state),
        stale_headers,
        Json(routed_delegate_start_request(
            parent.id.as_str(),
            &parent_conversation.id.as_str(),
        )),
    )
    .await
    .unwrap_err();
    assert_eq!(stale_error.0, axum::http::StatusCode::CONFLICT);
    assert!(stale_error.1 .0["error"]
        .as_str()
        .unwrap_or_default()
        .contains("is not the active caller run"));
}

#[tokio::test]
async fn test_delegate_wait_hydrates_the_jobs_exact_run_when_session_has_newer_run() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let delegated_session = state
        .app_state
        .delegated_session_repo
        .create(DelegatedSession::new(
            parent.project_id,
            "ideation".to_string(),
            parent.id.as_str().to_string(),
            "ralphx-general-explorer".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .expect("create delegated session");
    let delegated_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_delegation(
            delegated_session.id.clone(),
        ))
        .await
        .expect("create delegated conversation");

    let mut exact_run = AgentRun::new(delegated_conversation.id);
    exact_run.status = AgentRunStatus::Completed;
    exact_run.completed_at = Some(Utc::now());
    exact_run.effective_model_id = Some("exact-model".to_string());
    let exact_run = state
        .app_state
        .agent_run_repo
        .create(exact_run)
        .await
        .expect("create exact delegated run");

    let mut newer_run = AgentRun::new(delegated_conversation.id);
    newer_run.started_at = exact_run.started_at + chrono::Duration::seconds(1);
    newer_run.status = AgentRunStatus::Completed;
    newer_run.completed_at = Some(Utc::now());
    newer_run.effective_model_id = Some("newer-model".to_string());
    state
        .app_state
        .agent_run_repo
        .create(newer_run)
        .await
        .expect("create newer delegated run");

    let job_id = "job-exact-run".to_string();
    state
        .delegation_service
        .register_running(
            job_id.clone(),
            "ideation".to_string(),
            parent.id.as_str().to_string(),
            None,
            None,
            None,
            None,
            None,
            delegated_session.id.as_str().to_string(),
            Some(delegated_conversation.id.as_str()),
            Some(exact_run.id.as_str()),
            "ralphx-general-explorer".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            Some("exact-model".to_string()),
            None,
            None,
            None,
            None,
        )
        .await;

    let waited = wait_delegate(
        State(state),
        Json(DelegateWaitRequest {
            job_id: Some(job_id),
            job_ids: None,
            wait_timeout_ms: None,
            include_delegated_status: Some(true),
            include_child_status: None,
            include_messages: Some(false),
            message_limit: None,
        }),
    )
    .await
    .expect("wait response")
    .0;
    let hydrated_run = waited
        .delegated_status
        .and_then(|status| status.latest_run)
        .expect("exact delegated run status");
    assert_eq!(hydrated_run.agent_run_id, exact_run.id.as_str());
    assert_eq!(
        hydrated_run.effective_model_id.as_deref(),
        Some("exact-model")
    );
}

#[test]
fn test_build_delegated_task_started_payload_uses_parent_lineage_and_delegated_metadata() {
    let snapshot = DelegationJobSnapshot {
        job_id: "job-123".to_string(),
        parent_context_type: "ideation".to_string(),
        parent_context_id: "parent-session-1".to_string(),
        parent_turn_id: Some("turn-1".to_string()),
        parent_message_id: Some("msg-1".to_string()),
        parent_conversation_id: Some("parent-conv-1".to_string()),
        parent_agent_run_id: Some("parent-run-1".to_string()),
        parent_tool_use_id: Some("toolu-parent-1".to_string()),
        delegated_session_id: "delegated-session-1".to_string(),
        delegated_conversation_id: Some("delegated-conv-1".to_string()),
        delegated_agent_run_id: Some("run-1".to_string()),
        agent_name: "ralphx-execution-reviewer".to_string(),
        assignment: None,
        harness: "codex".to_string(),
        provider_session_id: Some("provider-thread-start".to_string()),
        upstream_provider: Some("openai".to_string()),
        provider_profile: Some("openai".to_string()),
        logical_model: Some("gpt-5.4".to_string()),
        effective_model_id: Some("gpt-5.4-2026-07-01".to_string()),
        logical_effort: Some("high".to_string()),
        effective_effort: Some("high".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        status: "running".to_string(),
        content: None,
        error: None,
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: None,
        history: vec![DelegationHistoryEntry {
            status: "running".to_string(),
            timestamp: "2026-04-12T10:00:00Z".to_string(),
            detail: None,
        }],
        delegated_status: None,
        timed_out: None,
    };

    let payload = build_delegated_task_started_payload(
        &snapshot,
        Some("gpt-5.4"),
        Some("high"),
        Some("never"),
        Some("danger-full-access"),
        42,
    )
    .expect("parent linkage should produce a payload");

    assert_eq!(payload.tool_use_id, "toolu-parent-1");
    assert_eq!(payload.run_id.as_deref(), Some("parent-run-1"));
    assert_eq!(payload.tool_name, "delegate_start");
    assert_eq!(
        payload.description.as_deref(),
        Some("ralphx-execution-reviewer")
    );
    assert_eq!(payload.subagent_type.as_deref(), Some("delegated"));
    assert_eq!(payload.delegated_job_id.as_deref(), Some("job-123"));
    assert_eq!(
        payload.delegated_session_id.as_deref(),
        Some("delegated-session-1")
    );
    assert_eq!(
        payload.delegated_conversation_id.as_deref(),
        Some("delegated-conv-1")
    );
    assert_eq!(payload.delegated_agent_run_id.as_deref(), Some("run-1"));
    assert_eq!(payload.provider_harness.as_deref(), Some("codex"));
    assert_eq!(
        payload.provider_session_id.as_deref(),
        Some("provider-thread-start")
    );
    assert_eq!(payload.logical_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(
        payload.effective_model_id.as_deref(),
        Some("gpt-5.4-2026-07-01")
    );
    assert_eq!(payload.logical_effort.as_deref(), Some("high"));
    assert_eq!(payload.approval_policy.as_deref(), Some("never"));
    assert_eq!(payload.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(payload.started_at.as_deref(), Some("2026-04-12T10:00:00Z"));
    assert_eq!(payload.completed_at, None);
    assert_eq!(
        payload.timestamp_provenance.as_deref(),
        Some("delegation_job")
    );
    assert_eq!(payload.conversation_id, "parent-conv-1");
    assert_eq!(payload.context_type, "ideation");
    assert_eq!(payload.context_id, "parent-session-1");
    assert_eq!(payload.seq, 42);
}

#[test]
fn test_build_delegated_task_completed_payload_uses_latest_run_attribution() {
    let snapshot = DelegationJobSnapshot {
        job_id: "job-456".to_string(),
        parent_context_type: "ideation".to_string(),
        parent_context_id: "parent-session-2".to_string(),
        parent_turn_id: Some("turn-2".to_string()),
        parent_message_id: Some("msg-2".to_string()),
        parent_conversation_id: Some("parent-conv-2".to_string()),
        parent_agent_run_id: Some("parent-run-2".to_string()),
        parent_tool_use_id: Some("toolu-parent-2".to_string()),
        delegated_session_id: "delegated-session-2".to_string(),
        delegated_conversation_id: Some("delegated-conv-2".to_string()),
        delegated_agent_run_id: Some("run-2".to_string()),
        agent_name: "ralphx-execution-reviewer".to_string(),
        assignment: None,
        harness: "codex".to_string(),
        provider_session_id: None,
        upstream_provider: None,
        provider_profile: None,
        logical_model: Some("gpt-5.4".to_string()),
        effective_model_id: None,
        logical_effort: Some("high".to_string()),
        effective_effort: None,
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        status: "running".to_string(),
        content: None,
        error: None,
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: None,
        history: vec![DelegationHistoryEntry {
            status: "running".to_string(),
            timestamp: "2026-04-12T10:00:00Z".to_string(),
            detail: None,
        }],
        delegated_status: None,
        timed_out: None,
    };
    let latest_run = DelegatedRunSummary {
        agent_run_id: "run-2".to_string(),
        status: "failed".to_string(),
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: Some("2026-04-12T10:00:05Z".to_string()),
        error_message: Some("Delegated reviewer failed validation".to_string()),
        harness: Some("codex".to_string()),
        provider_session_id: Some("provider-thread-1".to_string()),
        upstream_provider: Some("openai".to_string()),
        provider_profile: Some("openai".to_string()),
        logical_model: Some("gpt-5.4".to_string()),
        effective_model_id: Some("gpt-5.4".to_string()),
        logical_effort: Some("high".to_string()),
        effective_effort: Some("high".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        input_tokens: Some(9_116_803),
        output_tokens: Some(25_881),
        cache_creation_tokens: Some(0),
        cache_read_tokens: Some(8_837_504),
        processed_tokens: Some(9_142_684),
        estimated_usd: Some(0.12),
    };

    let payload = build_delegated_task_completed_payload(
        &snapshot,
        Some(&latest_run),
        "failed",
        Some("Delegated reviewer found a blocking issue"),
        Some("Delegated reviewer failed validation"),
        99,
    )
    .expect("parent linkage should produce a payload");

    assert_eq!(payload.tool_use_id, "toolu-parent-2");
    assert_eq!(payload.run_id.as_deref(), Some("parent-run-2"));
    assert_eq!(payload.agent_id.as_deref(), Some("run-2"));
    assert_eq!(payload.status.as_deref(), Some("failed"));
    assert_eq!(payload.total_duration_ms, Some(5000));
    assert_eq!(payload.total_tokens, Some(9_142_684));
    assert_eq!(payload.delegated_job_id.as_deref(), Some("job-456"));
    assert_eq!(
        payload.delegated_session_id.as_deref(),
        Some("delegated-session-2")
    );
    assert_eq!(
        payload.delegated_conversation_id.as_deref(),
        Some("delegated-conv-2")
    );
    assert_eq!(payload.delegated_agent_run_id.as_deref(), Some("run-2"));
    assert_eq!(payload.provider_harness.as_deref(), Some("codex"));
    assert_eq!(
        payload.provider_session_id.as_deref(),
        Some("provider-thread-1")
    );
    assert_eq!(payload.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(payload.provider_profile.as_deref(), Some("openai"));
    assert_eq!(payload.logical_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(payload.effective_model_id.as_deref(), Some("gpt-5.4"));
    assert_eq!(payload.logical_effort.as_deref(), Some("high"));
    assert_eq!(payload.effective_effort.as_deref(), Some("high"));
    assert_eq!(payload.approval_policy.as_deref(), Some("never"));
    assert_eq!(payload.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(payload.input_tokens, Some(9_116_803));
    assert_eq!(payload.output_tokens, Some(25_881));
    assert_eq!(payload.cache_creation_tokens, Some(0));
    assert_eq!(payload.cache_read_tokens, Some(8_837_504));
    assert_eq!(payload.estimated_usd, Some(0.12));
    assert_eq!(payload.started_at.as_deref(), Some("2026-04-12T10:00:00Z"));
    assert_eq!(
        payload.completed_at.as_deref(),
        Some("2026-04-12T10:00:05Z")
    );
    assert_eq!(
        payload.timestamp_provenance.as_deref(),
        Some("delegated_run")
    );
    assert_eq!(
        payload.text_output.as_deref(),
        Some("Delegated reviewer found a blocking issue")
    );
    assert_eq!(
        payload.error.as_deref(),
        Some("Delegated reviewer failed validation")
    );
    assert_eq!(payload.conversation_id, "parent-conv-2");
    assert_eq!(payload.context_type, "ideation");
    assert_eq!(payload.context_id, "parent-session-2");
    assert_eq!(payload.seq, 99);
}

#[test]
fn delegated_lifecycle_payload_uses_job_correlation_without_parent_tool_id() {
    let snapshot = DelegationJobSnapshot {
        job_id: "job-without-placement".to_string(),
        parent_context_type: "project".to_string(),
        parent_context_id: "project-1".to_string(),
        parent_turn_id: None,
        parent_message_id: None,
        parent_conversation_id: Some("parent-conversation".to_string()),
        parent_agent_run_id: None,
        parent_tool_use_id: None,
        delegated_session_id: "delegated-session".to_string(),
        delegated_conversation_id: Some("delegated-conversation".to_string()),
        delegated_agent_run_id: Some("delegated-run".to_string()),
        agent_name: "ralphx-general-explorer".to_string(),
        assignment: None,
        harness: "codex".to_string(),
        provider_session_id: None,
        upstream_provider: Some("openai".to_string()),
        provider_profile: None,
        logical_model: Some("gpt-5.4".to_string()),
        effective_model_id: None,
        logical_effort: Some("medium".to_string()),
        effective_effort: None,
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        status: "running".to_string(),
        content: None,
        error: None,
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: None,
        history: vec![],
        delegated_status: None,
        timed_out: None,
    };

    let started = build_delegated_task_started_payload(&snapshot, None, None, None, None, 7)
        .expect("parent conversation and job should be sufficient for lifecycle correlation");

    assert_eq!(started.tool_use_id, "delegate-job:job-without-placement");
    assert_eq!(
        started.delegated_job_id.as_deref(),
        Some("job-without-placement")
    );
    assert_eq!(started.conversation_id, "parent-conversation");
}

// ── Phase 1: backend-held bounded delegate_wait ──────────────────────────────

/// Seeds a delegated session + conversation + running agent run, registers a running
/// delegation job for it, and returns the job id plus the delegated run id.
async fn seed_running_delegation_job(
    state: &HttpServerState,
    parent: &IdeationSession,
    job_id: &str,
) -> (String, AgentRunId) {
    let delegated_session = state
        .app_state
        .delegated_session_repo
        .create(DelegatedSession::new(
            parent.project_id.clone(),
            "ideation".to_string(),
            parent.id.as_str().to_string(),
            "ralphx-general-explorer".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .expect("create delegated session");
    let delegated_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_delegation(
            delegated_session.id.clone(),
        ))
        .await
        .expect("create delegated conversation");
    let run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(delegated_conversation.id))
        .await
        .expect("create delegated run");

    state
        .delegation_service
        .register_running(
            job_id.to_string(),
            "ideation".to_string(),
            parent.id.as_str().to_string(),
            None,
            None,
            None,
            None,
            None,
            delegated_session.id.as_str().to_string(),
            Some(delegated_conversation.id.as_str()),
            Some(run.id.as_str()),
            "ralphx-general-explorer".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    (job_id.to_string(), run.id.clone())
}

fn wait_request(job_id: Option<&str>, job_ids: Option<Vec<&str>>) -> DelegateWaitRequest {
    DelegateWaitRequest {
        job_id: job_id.map(str::to_string),
        job_ids: job_ids.map(|ids| ids.into_iter().map(str::to_string).collect()),
        wait_timeout_ms: None,
        include_delegated_status: Some(false),
        include_child_status: None,
        include_messages: None,
        message_limit: None,
    }
}

/// Marks the registered job terminal through the same CAS the production settlement path uses,
/// which is the only thing allowed to fire the settlement watch signal.
async fn commit_job_terminal(state: &HttpServerState, job_id: &str, status: &str) {
    let candidate = state
        .delegation_service
        .terminal_candidate(job_id, status, Some("delegate output".to_string()), None)
        .await
        .expect("terminal candidate");
    assert!(
        state.delegation_service.commit_terminal(candidate).await,
        "commit_terminal should accept the first terminal for {job_id}"
    );
}

#[tokio::test]
async fn wait_delegate_without_timeout_returns_immediately() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let (job_id, _run) = seed_running_delegation_job(&state, &parent, "job-immediate").await;

    let started = std::time::Instant::now();
    let snapshot = wait_delegate(
        State(state.clone()),
        Json(wait_request(Some(&job_id), None)),
    )
    .await
    .expect("wait response")
    .0;

    assert_eq!(snapshot.status, "running");
    assert_eq!(
        snapshot.timed_out, None,
        "an immediate return must not report a timeout"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "default behavior must not block"
    );
}

#[tokio::test]
async fn wait_delegate_blocks_until_settlement() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let (job_id, _run) = seed_running_delegation_job(&state, &parent, "job-blocking").await;

    let settler_state = state.clone();
    let settler_job = job_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        commit_job_terminal(&settler_state, &settler_job, "completed").await;
    });

    let mut request = wait_request(Some(&job_id), None);
    request.wait_timeout_ms = Some(10_000);

    let started = std::time::Instant::now();
    let snapshot = wait_delegate(State(state.clone()), Json(request))
        .await
        .expect("wait response")
        .0;
    let elapsed = started.elapsed();

    assert_eq!(snapshot.status, "completed");
    assert_eq!(snapshot.timed_out, None);
    assert!(
        elapsed < Duration::from_secs(3),
        "block must return promptly after settlement, took {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(150),
        "block must actually wait for the settlement signal, took {elapsed:?}"
    );
}

#[tokio::test]
async fn wait_delegate_returns_timed_out_at_cap() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let (job_id, _run) = seed_running_delegation_job(&state, &parent, "job-timeout").await;

    let mut request = wait_request(Some(&job_id), None);
    request.wait_timeout_ms = Some(200);

    let snapshot = wait_delegate(State(state.clone()), Json(request))
        .await
        .expect("wait response")
        .0;

    assert_eq!(snapshot.timed_out, Some(true));
    assert_eq!(
        snapshot.status, "running",
        "a timeout must never settle the job"
    );
    assert_eq!(
        state
            .delegation_service
            .snapshot(&job_id)
            .await
            .expect("job still registered")
            .status,
        "running"
    );
}

#[test]
fn wait_delegate_clamps_timeout_below_the_stream_stall_guard() {
    let cap = ralphx_lib::infrastructure::agents::claude::delegation_config().wait_block_max_secs;
    let stall_guard =
        ralphx_lib::infrastructure::agents::claude::stream_timeouts().default_parse_stall_secs;

    // Config invariant: a legitimate backend-held block can never outlive the stall guard that
    // would kill the waiting coordinator's stream. This is the falsifiable guard against config
    // drift re-introducing the "blocking wait kills the coordinator" failure mode.
    assert!(
        cap < stall_guard,
        "delegation.wait_block_max_secs ({cap}) must stay below \
         timeouts.stream.default_parse_stall_secs ({stall_guard})"
    );

    let clamped = ralphx_lib::http_server::handlers::effective_wait_block(u64::MAX);
    assert!(
        clamped <= Duration::from_secs(cap),
        "an absurd caller timeout must clamp to the configured cap"
    );
    assert!(
        clamped < Duration::from_secs(stall_guard),
        "the effective block must stay strictly below the stall guard"
    );

    // A modest caller request is honored verbatim.
    assert_eq!(
        ralphx_lib::http_server::handlers::effective_wait_block(250),
        Duration::from_millis(250)
    );
}

#[tokio::test]
async fn wait_delegate_with_job_ids_returns_first_settled() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let (first, _) = seed_running_delegation_job(&state, &parent, "job-wave-1").await;
    let (second, _) = seed_running_delegation_job(&state, &parent, "job-wave-2").await;

    let settler_state = state.clone();
    let settler_job = second.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        commit_job_terminal(&settler_state, &settler_job, "completed").await;
    });

    let mut request = wait_request(None, Some(vec![&first, &second]));
    request.wait_timeout_ms = Some(10_000);

    let snapshot = wait_delegate(State(state.clone()), Json(request))
        .await
        .expect("wait response")
        .0;

    assert_eq!(
        snapshot.job_id, second,
        "the wave wait must return the job that actually settled"
    );
    assert_eq!(snapshot.status, "completed");
    assert_eq!(
        state
            .delegation_service
            .snapshot(&first)
            .await
            .expect("sibling still registered")
            .status,
        "running",
        "waking on one job must not settle its siblings"
    );
}

#[tokio::test]
async fn wait_delegate_rejects_both_job_id_and_job_ids() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let (job_id, _run) = seed_running_delegation_job(&state, &parent, "job-ambiguous").await;

    let mut request = wait_request(Some(&job_id), None);
    request.job_ids = Some(vec![job_id.clone()]);

    let error = wait_delegate(State(state.clone()), Json(request))
        .await
        .expect_err("ambiguous watch set must be rejected");
    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);

    let missing = wait_delegate(State(state.clone()), Json(wait_request(None, None)))
        .await
        .expect_err("empty watch set must be rejected");
    assert_eq!(missing.0, axum::http::StatusCode::BAD_REQUEST);

    let empty_list = wait_delegate(State(state), Json(wait_request(None, Some(vec![]))))
        .await
        .expect_err("empty job_ids must be rejected");
    assert_eq!(empty_list.0, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn wait_delegate_rejects_unknown_job_in_set() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_session(&state).await;
    let (job_id, _run) = seed_running_delegation_job(&state, &parent, "job-known").await;

    let error = wait_delegate(
        State(state),
        Json(wait_request(
            None,
            Some(vec![&job_id, "job-does-not-exist"]),
        )),
    )
    .await
    .expect_err("unknown job in the watch set must be rejected");
    assert_eq!(error.0, axum::http::StatusCode::NOT_FOUND);
}
