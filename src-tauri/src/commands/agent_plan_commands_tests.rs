use super::agent_plan_commands::{
    activate_agent_plan_direct_implementation_for_state as activate_agent_plan_direct_implementation_with_execution_state,
    activate_agent_task_pipeline_for_state as activate_agent_task_pipeline_with_execution_state,
    copy_agent_conversation_plan_for_state as copy_agent_conversation_plan_with_execution_state,
    import_agent_conversation_plan_for_state as import_agent_conversation_plan_with_execution_state,
    validate_complete_task_pipeline_proposal_selection, ActivateAgentPlanDirectImplementationInput,
    ActivateAgentTaskPipelineInput, CopyAgentConversationPlanInput,
    ImportAgentConversationPlanInput,
};
use crate::application::ideation_apply_service::apply_supervised_proposals_core;
use super::ideation_commands::ApplyProposalsInput;
use crate::application::{
    agent_conversation_workspace::resolve_agent_conversation_workspace_path,
    agent_task_pipeline_service::{
        validate_start_authority_sync, validate_supervised_task_pipeline,
    },
    interactive_process_registry::{InteractiveProcessKey, InteractiveProcessMetadata},
    AppState,
};
use crate::commands::ExecutionState;
use crate::domain::agents::{
    AgentHarnessKind, ManualRoleRuntimeOverride, ManualServiceTier, ProviderSessionRef,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunStatus, Artifact,
    ArtifactBucketId, ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactRelationType,
    ArtifactType, ChatConversation, CoordinationMode, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus, Priority, Project,
    ProposalCategory, TaskProposal,
};
use crate::domain::repositories::PlanApprovalActor;
use crate::domain::services::RunningAgentKey;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

async fn copy_agent_conversation_plan_for_state(
    input: CopyAgentConversationPlanInput,
    state: &AppState,
) -> Result<super::agent_plan_commands::AgentConversationPlanSeedResponse, String> {
    let execution_state = Arc::new(ExecutionState::new());
    copy_agent_conversation_plan_with_execution_state(input, state, &execution_state).await
}

async fn import_agent_conversation_plan_for_state(
    input: ImportAgentConversationPlanInput,
    state: &AppState,
) -> Result<super::agent_plan_commands::AgentConversationPlanSeedResponse, String> {
    let execution_state = Arc::new(ExecutionState::new());
    import_agent_conversation_plan_with_execution_state(input, state, &execution_state).await
}

async fn activate_agent_task_pipeline_for_state(
    input: ActivateAgentTaskPipelineInput,
    state: &AppState,
) -> Result<crate::commands::unified_chat_commands::AgentConversationWorkspaceResponse, String> {
    let execution_state = Arc::new(ExecutionState::new());
    activate_agent_task_pipeline_with_execution_state(input, state, &execution_state).await
}

async fn activate_agent_plan_direct_implementation_for_state(
    input: ActivateAgentPlanDirectImplementationInput,
    state: &AppState,
) -> Result<super::agent_plan_commands::ActivateAgentPlanDirectImplementationResponse, String> {
    let execution_state = Arc::new(ExecutionState::new());
    activate_agent_plan_direct_implementation_with_execution_state(input, state, &execution_state)
        .await
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo(root: &Path) {
    std::fs::create_dir_all(root).expect("repo root should be created");
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    std::fs::write(root.join("README.md"), "hello\n").expect("fixture file should be written");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

fn inline_plan(name: &str, content: &str, version: u32) -> Artifact {
    Artifact {
        id: ArtifactId::new(),
        artifact_type: ArtifactType::Specification,
        name: name.to_string(),
        content: ArtifactContent::inline(content),
        metadata: ArtifactMetadata::new("orchestrator").with_version(version),
        derived_from: vec![],
        bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
        archived_at: None,
    }
}

fn file_plan(name: &str, path: &str, version: u32) -> Artifact {
    Artifact {
        id: ArtifactId::new(),
        artifact_type: ArtifactType::Specification,
        name: name.to_string(),
        content: ArtifactContent::File {
            path: path.to_string(),
        },
        metadata: ArtifactMetadata::new("orchestrator").with_version(version),
        derived_from: vec![],
        bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
        archived_at: None,
    }
}

async fn seed_blueprint_for_session(state: &AppState, session_id: &str, content: &str) -> Artifact {
    let blueprint = state
        .artifact_repo
        .create(inline_plan("Implementation Blueprint", content, 1))
        .await
        .unwrap();
    let session_id = session_id.to_string();
    let blueprint_id = blueprint.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET plan_blueprint_artifact_id = ?2, plan_contract_version = 2
                 WHERE id = ?1",
                rusqlite::params![session_id, blueprint_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    blueprint
}

async fn setup_target_workspace(
    mode: AgentConversationWorkspaceMode,
) -> (AppState, Project, ChatConversation, TempDir) {
    let state = AppState::new_sqlite_for_apply_test();
    let test_root = tempfile::tempdir().expect("test root should be created");
    let project_dir = test_root.path().join("project");
    let worktree_parent = test_root.path().join("worktrees");
    std::fs::create_dir_all(&worktree_parent).unwrap();
    setup_repo(&project_dir);
    let mut project = Project::new(
        "Agent plan test".to_string(),
        project_dir.to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().into_owned());
    let project = state.project_repo.create(project).await.unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(mode));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    seed_sql_conversation_projection(&state, &conversation, mode).await;
    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation.id).unwrap();
    std::fs::create_dir_all(workspace_path.parent().unwrap()).unwrap();
    let workspace_path_arg = workspace_path.to_string_lossy().to_string();
    git(
        &project_dir,
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/test/agent-plan",
            workspace_path_arg.as_str(),
            "main",
        ],
    );
    let workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/agent-plan".to_string(),
        workspace_path.to_string_lossy().into_owned(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    (state, project, conversation, test_root)
}

async fn seed_sql_conversation_projection(
    state: &AppState,
    conversation: &ChatConversation,
    mode: AgentConversationWorkspaceMode,
) {
    let conversation_id = conversation.id.as_str().to_string();
    let context_id = conversation.context_id.clone();
    let agent_mode = mode.to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
                 VALUES (?1, 'project', ?2, ?3)",
                rusqlite::params![conversation_id, context_id, agent_mode],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

async fn set_sql_conversation_mode(
    state: &AppState,
    conversation: &ChatConversation,
    mode: AgentConversationWorkspaceMode,
) {
    let conversation_id = conversation.id.as_str().to_string();
    let agent_mode = mode.to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE chat_conversations SET agent_mode = ?2 WHERE id = ?1",
                rusqlite::params![conversation_id, agent_mode],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

async fn seed_planning_provider_session(state: &AppState, conversation: &ChatConversation) {
    state
        .chat_conversation_repo
        .update_provider_session_ref(
            &conversation.id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "planning-session".to_string(),
            },
        )
        .await
        .expect("planning provider session should persist");
}

async fn seed_source_plan(
    state: &AppState,
    project: &Project,
) -> (IdeationSession, Artifact, Artifact) {
    let source_v1 = state
        .artifact_repo
        .create(inline_plan("Source plan", "# Source v1", 1))
        .await
        .unwrap();
    let source_v2 = state
        .artifact_repo
        .create_with_previous_version(
            Artifact {
                id: ArtifactId::new(),
                artifact_type: ArtifactType::Specification,
                name: "Source plan".to_string(),
                content: ArtifactContent::inline("# Source v2"),
                metadata: ArtifactMetadata::new("orchestrator").with_version(2),
                derived_from: vec![],
                bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
                archived_at: None,
            },
            source_v1.id.clone(),
        )
        .await
        .unwrap();
    let source_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .plan_artifact_id(source_v2.id.clone())
                .plan_contract_version(1)
                .build(),
        )
        .await
        .unwrap();
    (source_session, source_v1, source_v2)
}

#[tokio::test]
async fn copy_agent_conversation_plan_clones_complete_v2_bundle_and_provenance() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let (mut source_session, _source_v1, source_overview) =
        seed_source_plan(&state, &project).await;
    let source_blueprint =
        seed_blueprint_for_session(&state, source_session.id.as_str(), "# Source blueprint").await;
    source_session = state
        .ideation_session_repo
        .get_by_id(&source_session.id)
        .await
        .unwrap()
        .unwrap();

    let response = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_overview.id.as_str().to_string(),
            source_version: source_overview.metadata.version,
        },
        &state,
    )
    .await
    .unwrap();

    let copied_blueprint = response
        .blueprint_artifact
        .expect("complete v2 copy must return its Blueprint");
    assert_eq!(copied_blueprint.content, "# Source blueprint");
    assert_eq!(
        copied_blueprint.derived_from,
        vec![source_blueprint.id.as_str().to_string()]
    );

    let target_session = state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(response.session_id.clone()))
        .await
        .unwrap()
        .unwrap();
    let target_bundle = target_session
        .plan_artifact_bundle()
        .expect("copied target must contain a complete bundle");
    assert_eq!(target_bundle.overview_id.as_str(), response.artifact.id);
    assert_eq!(
        target_bundle.blueprint_id.as_ref().map(ArtifactId::as_str),
        Some(copied_blueprint.id.as_str())
    );

    let relations = state
        .artifact_repo
        .get_relations(&ArtifactId::from_string(response.artifact.id))
        .await
        .unwrap();
    assert!(relations.iter().any(|relation| {
        relation.relation_type == ArtifactRelationType::RelatedTo
            && relation.to_artifact_id.as_str() == copied_blueprint.id
    }));
}

#[tokio::test]
async fn import_agent_conversation_plan_switches_to_plan_and_creates_draft() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;

    let response = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Imported plan".to_string(),
            content: "# Imported plan".to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.workspace.mode, "plan");
    assert_eq!(response.artifact.name, "Imported plan");
    assert_eq!(response.artifact.content, "# Imported plan");
    assert_eq!(response.artifact.version, 1);
    assert_eq!(
        response.artifact.plan_approval_status.as_deref(),
        Some("draft")
    );

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    let linked_session_id = workspace.linked_ideation_session_id.unwrap();
    assert_eq!(linked_session_id.as_str(), response.session_id);

    let session = state
        .ideation_session_repo
        .get_by_id(&linked_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
    assert_eq!(
        session.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(response.artifact.id.as_str()),
    );
}

#[tokio::test]
async fn approved_current_plan_activates_durable_tasks_pipeline_once() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Approved plan".to_string(),
            content: "# Approved".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                crate::domain::entities::IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;

    let input = || ActivateAgentTaskPipelineInput {
        conversation_id: conversation.id.as_str().to_string(),
        session_id: seeded.session_id.clone(),
        runtime_override: None,
    };
    let activated = activate_agent_task_pipeline_for_state(input(), &state)
        .await
        .unwrap();
    let replayed = activate_agent_task_pipeline_for_state(input(), &state)
        .await
        .unwrap();

    assert_eq!(activated.mode, "tasks");
    assert_eq!(
        activated.task_pipeline_session_id.as_deref(),
        Some(seeded.session_id.as_str()),
    );
    assert!(activated.task_pipeline_available);
    assert_eq!(
        replayed.task_pipeline_session_id,
        activated.task_pipeline_session_id,
    );
    assert!(
        state
            .task_repo
            .get_by_ideation_session(&crate::domain::entities::IdeationSessionId::from_string(
                seeded.session_id
            ),)
            .await
            .unwrap()
            .is_empty(),
        "Create Proposals authority must not create Kanban tasks",
    );
}

#[tokio::test]
async fn direct_implementation_rejects_stale_blueprint_before_atomic_mode_switch() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Direct implementation".to_string(),
            content: "# Overview".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint v1").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint v2").await;
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;
    seed_planning_provider_session(&state, &conversation).await;

    let input = || ActivateAgentPlanDirectImplementationInput {
        conversation_id: conversation.id.as_str().to_string(),
        session_id: seeded.session_id.clone(),
        retry: false,
    };
    let error = activate_agent_plan_direct_implementation_for_state(input(), &state)
        .await
        .unwrap_err();
    assert!(error.contains("blueprint version requires explicit user approval"));
    assert!(
        state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .expect("conversation should persist")
            .provider_session_ref()
            .is_some(),
        "authority rejection must not clear the planning provider session"
    );
    let unchanged = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.mode, AgentConversationWorkspaceMode::Plan);

    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    let activated = activate_agent_plan_direct_implementation_for_state(input(), &state)
        .await
        .unwrap();
    assert_eq!(activated.workspace.mode, "edit");
    assert!(
        state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .expect("conversation should persist")
            .provider_session_ref()
            .is_none(),
        "committed activation must clear the planning provider session"
    );
    assert_eq!(activated.artifact_references.len(), 2);
    assert_eq!(
        activated.artifact_references[0].artifact_id,
        seeded.artifact.id
    );
    assert_eq!(
        activated.artifact_references[1].title.as_deref(),
        Some("Implementation Blueprint")
    );
    assert_eq!(activated.artifact_references[0].kind, "plan");
    assert_eq!(activated.artifact_references[1].kind, "plan_blueprint");
    assert!(!activated.plan_context_fingerprint.is_empty());

    let retry = activate_agent_plan_direct_implementation_for_state(
        ActivateAgentPlanDirectImplementationInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            retry: true,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(retry.workspace.mode, "edit");
    assert_eq!(retry.artifact_references, activated.artifact_references);
    assert_eq!(
        retry.plan_context_fingerprint,
        activated.plan_context_fingerprint
    );
}

#[tokio::test]
async fn direct_implementation_stops_plan_runtime_before_activation_handoff() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Runtime handoff plan".to_string(),
            content: "# Overview".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;
    seed_planning_provider_session(&state, &conversation).await;

    let run = AgentRun::new(conversation.id);
    let agent_run_id = run.id.clone();
    let run_id = agent_run_id.as_str().to_string();
    state.agent_run_repo.create(run).await.unwrap();
    let interactive_key = InteractiveProcessKey::new("project", conversation.id.as_str());
    let running_key = RunningAgentKey::new("project", conversation.id.as_str());
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn Plan runtime fixture");
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("Plan runtime stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some(run_id.clone()),
                agent_name: Some("ralphx:ralphx-ideation".to_string()),
                agent_profile: Some("plan".to_string()),
                ..Default::default()
            },
        )
        .await;
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation.id.as_str(),
            run_id,
            None,
            None,
        )
        .await;

    let activated = activate_agent_plan_direct_implementation_for_state(
        ActivateAgentPlanDirectImplementationInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            retry: false,
        },
        &state,
    )
    .await
    .expect("activation should stop and retire the Plan runtime");

    assert_eq!(activated.workspace.mode, "edit");
    assert!(
        !state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await
    );
    assert!(!state.running_agent_registry.is_running(&running_key).await);
    let stopped_run = state
        .agent_run_repo
        .get_by_id(&agent_run_id)
        .await
        .unwrap()
        .expect("stopped Plan run should persist");
    assert_eq!(stopped_run.status, AgentRunStatus::Failed);
    assert_eq!(
        stopped_run.error_message.as_deref(),
        Some("Agent stopped by user"),
        "the command must stop the running Plan agent before the mode CAS"
    );
    assert!(
        state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .expect("conversation should persist")
            .provider_session_ref()
            .is_none(),
        "post-commit handoff must clear the planning provider session"
    );

    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    let _ = child.kill().await;
}

#[tokio::test]
async fn direct_implementation_authority_conflict_keeps_provider_session() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "CAS conflict plan".to_string(),
            content: "# Overview".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .expect("workspace should exist");
    workspace.mode = AgentConversationWorkspaceMode::Edit;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    seed_planning_provider_session(&state, &conversation).await;
    let run = AgentRun::new(conversation.id);
    let run_id = run.id.as_str().to_string();
    state.agent_run_repo.create(run).await.unwrap();
    let interactive_key = InteractiveProcessKey::new("project", conversation.id.as_str());
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn Plan runtime fixture");
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("Plan runtime stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some(run_id.clone()),
                agent_name: Some("ralphx:ralphx-ideation".to_string()),
                agent_profile: Some("plan".to_string()),
                ..Default::default()
            },
        )
        .await;
    let running_key = RunningAgentKey::new("project", conversation.id.as_str());
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation.id.as_str(),
            run_id.clone(),
            None,
            None,
        )
        .await;

    let error = activate_agent_plan_direct_implementation_for_state(
        ActivateAgentPlanDirectImplementationInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            retry: false,
        },
        &state,
    )
    .await
    .expect_err("stale workspace mode must reject the activation CAS");

    assert!(error.contains("Plan conversation no longer owns this planning session"));
    assert_eq!(
        state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .expect("conversation should persist")
            .agent_mode,
        Some(AgentConversationWorkspaceMode::Plan),
        "authority conflict must leave the conversation in Plan mode"
    );
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "authority preflight must not retire the Plan runtime"
    );
    assert!(
        state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .expect("conversation should persist")
            .provider_session_ref()
            .is_some(),
        "authority conflict must leave the planning provider session intact"
    );
    state
        .interactive_process_registry
        .remove(&interactive_key)
        .await;
    state
        .running_agent_registry
        .unregister(&running_key, &run_id)
        .await;
    let _ = child.kill().await;
}

#[tokio::test]
async fn task_pipeline_activation_atomically_applies_explicit_role_bindings() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Runtime-bound plan".to_string(),
            content: "# Runtime-bound plan".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;
    let conversation_id = conversation.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE chat_conversations
                 SET coordination_mode = 'rx_native_team',
                     persona_id = 'stale-workspace-persona'
                 WHERE id = ?1",
                [conversation_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            runtime_override: Some(ManualRoleRuntimeOverride {
                harness: AgentHarnessKind::Claude,
                model: None,
                effort: None,
                service_tier: ManualServiceTier::ProviderDefault,
                coordination_mode: Some(CoordinationMode::Solo),
                persona_id: None,
            }),
        },
        &state,
    )
    .await
    .unwrap();

    let conversation_id = conversation.id.as_str().to_string();
    let stored = state
        .db
        .run(move |conn| {
            Ok(conn.query_row(
                "SELECT agent_mode, coordination_mode, persona_id
                 FROM chat_conversations WHERE id = ?1",
                [conversation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )?)
        })
        .await
        .unwrap();
    assert_eq!(stored.0, "tasks");
    assert_eq!(stored.1, "solo");
    assert!(stored.2.is_none());
}

#[tokio::test]
async fn disabled_tasks_reject_pipeline_activation_without_attaching_workspace() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Disabled Tasks plan".to_string(),
            content: "# Disabled Tasks".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    state
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE ideation_settings
                 SET tasks_enabled = 0, tasks_feature_state = 'disabled'
                 WHERE id = 1",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let error = activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect_err("Tasks OFF must reject new pipeline activation");
    assert!(error.starts_with("ralphx:tasks_disabled"));

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    assert!(workspace.task_pipeline_session_id.is_none());
}

#[tokio::test]
async fn activation_write_failure_cannot_advance_only_the_conversation_mode() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Atomic activation".to_string(),
            content: "# Atomic activation".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                crate::domain::entities::IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;
    state
        .db
        .run(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER fail_tasks_conversation_activation
                 BEFORE UPDATE OF agent_mode ON chat_conversations
                 WHEN NEW.agent_mode = 'tasks'
                 BEGIN SELECT RAISE(FAIL, 'conversation activation failed'); END;",
            )?;
            Ok(())
        })
        .await
        .unwrap();

    activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            runtime_override: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    let stored_conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored_conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Plan),
        "failed activation must not leave the conversation projection in Tasks",
    );
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    assert!(workspace.task_pipeline_session_id.is_none());
}

#[tokio::test]
async fn stale_conversation_projection_cannot_activate_tasks_pipeline() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Stale projection".to_string(),
            content: "# Stale projection".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let session_id = seeded.session_id.clone();
    let artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                crate::domain::entities::IdeationSessionId::from_string(session_id),
                Some(&artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();

    let error = activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            runtime_override: None,
        },
        &state,
    )
    .await
    .unwrap_err();
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        error,
        "Conflict: Task pipeline conversation projection changed before activation",
    );
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    assert!(workspace.task_pipeline_session_id.is_none());
}

#[tokio::test]
async fn unapproved_plan_cannot_activate_tasks_pipeline() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Draft plan".to_string(),
            content: "# Draft".to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    let error = activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            runtime_override: None,
        },
        &state,
    )
    .await
    .unwrap_err();
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(error, "Current plan is not approved");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    assert!(workspace.task_pipeline_session_id.is_none());
}

#[tokio::test]
async fn stale_plan_approval_cannot_activate_tasks_pipeline() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Approved then revised plan".to_string(),
            content: "# Version one".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint v1").await;
    let approval_session_id = seeded.session_id.clone();
    let approval_artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                crate::domain::entities::IdeationSessionId::from_string(approval_session_id),
                Some(&approval_artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    let previous_id = ArtifactId::from_string(seeded.artifact.id.clone());
    let mut revised = state
        .artifact_repo
        .get_by_id(&previous_id)
        .await
        .unwrap()
        .unwrap();
    revised.id = ArtifactId::new();
    revised.content = ArtifactContent::Inline {
        text: "# Version two".to_string(),
    };
    revised.metadata.version += 1;
    state
        .artifact_repo
        .create_with_previous_version(revised, previous_id)
        .await
        .unwrap();

    let error = activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id,
            runtime_override: None,
        },
        &state,
    )
    .await
    .unwrap_err();
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(error, "Current plan version is not approved");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    assert!(workspace.task_pipeline_session_id.is_none());
}

#[tokio::test]
async fn start_tasks_requires_the_complete_current_proposal_set() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Proposal selection".to_string(),
            content: "# Proposal selection".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    let session_id =
        crate::domain::entities::IdeationSessionId::from_string(seeded.session_id.clone());
    let first = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session_id.clone(),
            "First",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();
    let second = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session_id,
            "Second",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();

    let subset_error = validate_complete_task_pipeline_proposal_selection(
        &state,
        &seeded.session_id,
        &[first.id.as_str().to_string()],
    )
    .await
    .unwrap_err();
    assert!(subset_error.contains("complete current proposal set"));
    validate_complete_task_pipeline_proposal_selection(
        &state,
        &seeded.session_id,
        &[
            first.id.as_str().to_string(),
            second.id.as_str().to_string(),
        ],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn task_pipeline_validation_rejects_empty_stale_and_non_user_authority() {
    let empty_state = AppState::new_sqlite_for_apply_test();
    let empty_error =
        validate_complete_task_pipeline_proposal_selection(&empty_state, "missing-session", &[])
            .await
            .unwrap_err();
    assert_eq!(empty_error, "Task pipeline has no proposals to start");

    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Authority validation".to_string(),
            content: "# Authority validation".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;

    let wrong_mode = validate_supervised_task_pipeline(
        &state,
        &conversation.id.as_str(),
        &seeded.session_id,
        AgentConversationWorkspaceMode::Tasks,
    )
    .await
    .unwrap_err();
    assert_eq!(wrong_mode, "Agent workspace must be in tasks mode");

    let wrong_attachment = validate_supervised_task_pipeline(
        &state,
        &conversation.id.as_str(),
        "different-session",
        AgentConversationWorkspaceMode::Plan,
    )
    .await
    .unwrap_err();
    assert_eq!(
        wrong_attachment,
        "Task pipeline session does not belong to this conversation"
    );

    let approval_session_id = seeded.session_id.clone();
    let approval_artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(approval_session_id),
                Some(&approval_artifact_id),
                PlanApprovalActor::Judge,
            )
            .map(|_| ())
        })
        .await
        .unwrap();

    let non_user_approval = validate_supervised_task_pipeline(
        &state,
        &conversation.id.as_str(),
        &seeded.session_id,
        AgentConversationWorkspaceMode::Plan,
    )
    .await
    .unwrap_err();
    assert_eq!(
        non_user_approval,
        "Current plan requires explicit user approval"
    );
}

#[tokio::test]
async fn start_authority_rejects_stale_state_and_accepts_restored_exact_state() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Transactional start authority".to_string(),
            content: "# Transactional start authority".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let approval_session_id = seeded.session_id.clone();
    let approval_artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(approval_session_id),
                Some(&approval_artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;
    activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id.clone(),
            runtime_override: None,
        },
        &state,
    )
    .await
    .unwrap();

    let session_id = IdeationSessionId::from_string(seeded.session_id.clone());
    let first = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session_id.clone(),
            "First authority proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();
    let second = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session_id.clone(),
            "Second authority proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();
    let conversation_id = conversation.id.as_str().to_string();
    let session_id_string = seeded.session_id.clone();
    let first_id = first.id.as_str().to_string();
    let proposal_ids = vec![first_id.clone(), second.id.as_str().to_string()];

    let stale_selection = state
        .db
        .run({
            let conversation_id = conversation_id.clone();
            let session_id_string = session_id_string.clone();
            move |conn| {
                validate_start_authority_sync(
                    conn,
                    &conversation_id,
                    &session_id_string,
                    &[first_id],
                )
            }
        })
        .await
        .unwrap_err();
    assert!(stale_selection
        .to_string()
        .contains("Task proposals changed after review"));

    let unacknowledged = state
        .db
        .run({
            let conversation_id = conversation_id.clone();
            let session_id_string = session_id_string.clone();
            let proposal_ids = proposal_ids.clone();
            move |conn| {
                validate_start_authority_sync(
                    conn,
                    &conversation_id,
                    &session_id_string,
                    &proposal_ids,
                )
            }
        })
        .await
        .unwrap_err();
    assert!(unacknowledged
        .to_string()
        .contains("dependencies must be reviewed"));

    state
        .ideation_session_repo
        .update_status(&session_id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();
    let inactive = state
        .db
        .run({
            let conversation_id = conversation_id.clone();
            let session_id_string = session_id_string.clone();
            let proposal_ids = proposal_ids.clone();
            move |conn| {
                validate_start_authority_sync(
                    conn,
                    &conversation_id,
                    &session_id_string,
                    &proposal_ids,
                )
            }
        })
        .await
        .unwrap_err();
    assert!(inactive
        .to_string()
        .contains("no longer an active planning session"));

    state
        .ideation_session_repo
        .update_status(&session_id, IdeationSessionStatus::Active)
        .await
        .unwrap();
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(&seeded.session_id)
        .await
        .unwrap();
    state
        .db
        .run({
            let session_id_string = session_id_string.clone();
            move |conn| {
                conn.execute(
                    "UPDATE plan_artifact_approvals SET approved_by = 'judge' WHERE session_id = ?1",
                    [session_id_string],
                )?;
                Ok(())
            }
        })
        .await
        .unwrap();
    let non_user = state
        .db
        .run({
            let conversation_id = conversation_id.clone();
            let session_id_string = session_id_string.clone();
            let proposal_ids = proposal_ids.clone();
            move |conn| {
                validate_start_authority_sync(
                    conn,
                    &conversation_id,
                    &session_id_string,
                    &proposal_ids,
                )
            }
        })
        .await
        .unwrap_err();
    assert!(non_user
        .to_string()
        .contains("requires explicit user approval"));

    state
        .db
        .run({
            let session_id_string = session_id_string.clone();
            move |conn| {
                conn.execute(
                    "UPDATE plan_artifact_approvals SET approved_by = 'user' WHERE session_id = ?1",
                    [session_id_string],
                )?;
                Ok(())
            }
        })
        .await
        .unwrap();
    state
        .db
        .run({
            let proposal_ids = proposal_ids.clone();
            move |conn| {
                validate_start_authority_sync(
                    conn,
                    &conversation_id,
                    &session_id_string,
                    &proposal_ids,
                )
            }
        })
        .await
        .unwrap();

    assert!(state
        .task_repo
        .get_by_ideation_session(&session_id)
        .await
        .unwrap()
        .is_empty());
    assert!(state
        .execution_plan_repo
        .get_by_session(&session_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn supervised_apply_requires_the_owning_tasks_conversation() {
    let (state, _project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let seeded = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Supervised apply".to_string(),
            content: "# Supervised apply".to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    seed_blueprint_for_session(&state, &seeded.session_id, "# Blueprint").await;
    let approval_session_id = seeded.session_id.clone();
    let approval_artifact_id = seeded.artifact.id.clone();
    state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                crate::domain::entities::IdeationSessionId::from_string(approval_session_id),
                Some(&approval_artifact_id),
                PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .unwrap();
    set_sql_conversation_mode(&state, &conversation, AgentConversationWorkspaceMode::Plan).await;
    activate_agent_task_pipeline_for_state(
        ActivateAgentTaskPipelineInput {
            conversation_id: conversation.id.as_str().to_string(),
            session_id: seeded.session_id.clone(),
            runtime_override: None,
        },
        &state,
    )
    .await
    .unwrap();

    let session_id =
        crate::domain::entities::IdeationSessionId::from_string(seeded.session_id.clone());
    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session_id.clone(),
            "Owned proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .unwrap();
    let input = || ApplyProposalsInput {
        session_id: seeded.session_id.clone(),
        proposal_ids: vec![proposal.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };
    let execution_state = Arc::new(ExecutionState::new());

    let error = apply_supervised_proposals_core(
        &state,
        &execution_state,
        input(),
        "different-conversation".to_string(),
    )
    .await
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("Tasks conversation no longer owns this pipeline"));
    assert!(state
        .task_repo
        .get_by_ideation_session(&session_id)
        .await
        .unwrap()
        .is_empty());

    let result = apply_supervised_proposals_core(
        &state,
        &execution_state,
        input(),
        conversation.id.as_str().to_string(),
    )
    .await
    .unwrap();
    assert_eq!(result.tasks_created, 1);
    assert_eq!(
        state
            .task_repo
            .get_by_ideation_session(&session_id)
            .await
            .unwrap()
            .len(),
        2,
        "one proposal task and its merge task should be created",
    );
}

#[tokio::test]
async fn import_agent_conversation_plan_rejects_blank_fields_before_switching() {
    let state = AppState::new_sqlite_test();

    let title_error = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: "conversation-blank-title".to_string(),
            title: "   ".to_string(),
            content: "# Plan".to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(title_error, "Plan title is required");

    let content_error = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: "conversation-blank-content".to_string(),
            title: "Imported plan".to_string(),
            content: "\n\t ".to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(content_error, "Plan content is required");
}

#[tokio::test]
async fn copy_agent_conversation_plan_rejects_zero_source_version_before_lookup() {
    let state = AppState::new_sqlite_test();

    let error = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: "conversation-zero-version".to_string(),
            source_session_id: "source-session".to_string(),
            source_artifact_id: "source-artifact".to_string(),
            source_version: 0,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error, "Source plan version must be greater than zero");
}

#[tokio::test]
async fn copy_agent_conversation_plan_uses_selected_source_version() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let (source_session, source_v1, source_v2) = seed_source_plan(&state, &project).await;

    let response = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 1,
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.artifact.content, "# Source v1");
    assert_eq!(
        response.artifact.derived_from,
        vec![source_v1.id.as_str().to_string()]
    );
    assert_eq!(
        response.artifact.plan_approval_status.as_deref(),
        Some("draft")
    );

    let source_session_after = state
        .ideation_session_repo
        .get_by_id(&source_session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source_session_after.plan_artifact_id, Some(source_v2.id));

    let relations = state
        .artifact_repo
        .get_relations(&ArtifactId::from_string(response.artifact.id.clone()))
        .await
        .unwrap();
    assert!(relations.iter().any(|relation| {
        relation.relation_type == ArtifactRelationType::DerivedFrom
            && relation.to_artifact_id == source_v1.id
    }));
}

#[tokio::test]
async fn copy_agent_conversation_plan_rejects_historical_v2_overview_without_exact_pair() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let (source_session, _source_v1, source_v2) = seed_source_plan(&state, &project).await;
    seed_blueprint_for_session(
        &state,
        source_session.id.as_str(),
        "# Current source blueprint",
    )
    .await;

    let error = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 1,
        },
        &state,
    )
    .await
    .expect_err("historical v2 Overview cannot prove its exact Blueprint pair");

    assert_eq!(
        error,
        "Historical v2 plan copies require selecting the current Overview and Blueprint pair"
    );
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Edit);
    assert!(workspace.linked_ideation_session_id.is_none());
}

#[tokio::test]
async fn copy_agent_conversation_plan_rejects_file_backed_source_plan() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Edit).await;
    let source_plan = state
        .artifact_repo
        .create(file_plan("File source plan", "/tmp/source-plan.md", 1))
        .await
        .unwrap();
    let source_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .plan_artifact_id(source_plan.id.clone())
                .plan_contract_version(1)
                .build(),
        )
        .await
        .unwrap();

    let error = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_plan.id.as_str().to_string(),
            source_version: 1,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        "File-backed source plans cannot be copied from the agent Plan tab"
    );
}

#[tokio::test]
async fn copy_agent_conversation_plan_over_existing_target_adds_local_version() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Plan).await;
    let (source_session, _source_v1, source_v2) = seed_source_plan(&state, &project).await;

    let target_v1 = state
        .artifact_repo
        .create(inline_plan("Target plan", "# Target v1", 1))
        .await
        .unwrap();
    let mut target_session = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .plan_artifact_id(target_v1.id.clone())
        .source_context_type("agent_conversation")
        .source_context_id(conversation.id.as_str())
        .build();
    target_session = state
        .ideation_session_repo
        .create(target_session)
        .await
        .unwrap();
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    workspace.linked_ideation_session_id = Some(target_session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let response = copy_agent_conversation_plan_for_state(
        CopyAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            source_session_id: source_session.id.as_str().to_string(),
            source_artifact_id: source_v2.id.as_str().to_string(),
            source_version: 2,
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.session_id, target_session.id.as_str());
    assert_eq!(response.artifact.content, "# Source v2");
    assert_eq!(response.artifact.version, 2);

    let refreshed_session = state
        .ideation_session_repo
        .get_by_id(&target_session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        refreshed_session
            .plan_artifact_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(response.artifact.id.as_str()),
    );

    let history = state
        .artifact_repo
        .get_version_history(&ArtifactId::from_string(response.artifact.id.clone()))
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version, 2);
    assert_eq!(history[1].id, target_v1.id);
}

#[tokio::test]
async fn import_agent_conversation_plan_rejects_accepted_target_session() {
    let (state, project, conversation, _test_root) =
        setup_target_workspace(AgentConversationWorkspaceMode::Plan).await;
    let target_session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation.id.as_str())
                .build(),
        )
        .await
        .unwrap();
    state
        .ideation_session_repo
        .update_status(&target_session.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    workspace.linked_ideation_session_id = Some(target_session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let error = import_agent_conversation_plan_for_state(
        ImportAgentConversationPlanInput {
            conversation_id: conversation.id.as_str().to_string(),
            title: "Imported plan".to_string(),
            content: "# Imported plan".to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        "Validation error: Cannot modify accepted session. Reopen it first."
    );
}
