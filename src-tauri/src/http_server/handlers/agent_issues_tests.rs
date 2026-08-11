use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationIssueCanonicalIdentity, AgentConversationWorkspace,
    AgentConversationWorkspaceMode, AgentWorkspaceFollowupProvenance, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSessionId, ProjectId, Task,
    AGENT_CONVERSATION_ISSUE_DEDUPE_CANDIDATE_ATTACHED,
    AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW, AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED,
    AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED, AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED,
    AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED,
};
use crate::domain::review::ReviewSettings;
use crate::http_server::types::HttpServerState;
use axum::{extract::State, Json};
use std::sync::Arc;

fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState::new_test(app_state)
}

fn issue_request(origin_conversation_id: Option<String>) -> RegisterAgentConversationIssueRequest {
    RegisterAgentConversationIssueRequest {
        origin_conversation_id,
        source_task_id: Some(" task-1 ".to_string()),
        source_context_type: Some(" review ".to_string()),
        source_context_id: Some(" review-1 ".to_string()),
        source_agent_name: Some(" ralphx-execution-reviewer ".to_string()),
        issue_kind: "Plan Drift!".to_string(),
        severity: Some("High impact".to_string()),
        blocking_scope: Some("Follow-up only".to_string()),
        title: "Investigate unrelated drift".to_string(),
        summary: "The task uncovered unrelated work.".to_string(),
        evidence: Some("src/lib.rs".to_string()),
        recommendation: Some("Open a separate branch conversation.".to_string()),
        blocker_fingerprint: Some(" drift:task-1 ".to_string()),
        attach_to_issue_id: None,
        confirm_new: false,
        new_issue_reason: None,
        issue_check_token: None,
        followup_title: Some("Follow up on drift".to_string()),
        followup_prompt: Some("Decide how to handle unrelated drift.".to_string()),
        auto_followup_eligible: false,
        provider_harness: None,
        model_override: None,
        logical_effort: None,
    }
}

fn origin_only_issue_request(
    origin_conversation_id: Option<String>,
) -> RegisterAgentConversationIssueRequest {
    let mut req = issue_request(origin_conversation_id);
    req.source_task_id = None;
    req.source_context_type = None;
    req.source_context_id = None;
    req
}

fn test_workspace(
    conversation: &ChatConversation,
    project_id: &ProjectId,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        format!("agent/{}", conversation.id.as_str()),
        "/tmp/ralphx-issue-test".to_string(),
    )
}

async fn seed_project_conversation(app_state: &AppState) -> (ProjectId, ChatConversation) {
    let project_id = ProjectId::new();
    let conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    (project_id, conversation)
}

async fn seed_source_task(
    app_state: &AppState,
    project_id: &ProjectId,
    session_id: Option<IdeationSessionId>,
) -> Task {
    let mut task = Task::new(project_id.clone(), "Source task".to_string());
    task.ideation_session_id = session_id;
    app_state.task_repo.create(task).await.unwrap()
}

async fn register_issue_ok(
    state: HttpServerState,
    req: RegisterAgentConversationIssueRequest,
) -> RegisterAgentConversationIssueResponse {
    match register_agent_issue(State(state), Json(req)).await {
        Ok(Json(response)) => response,
        Err((status, body)) => panic!("expected successful issue registration: {status} {body:?}"),
    }
}

async fn register_issue_error(
    state: HttpServerState,
    req: RegisterAgentConversationIssueRequest,
) -> JsonError {
    register_agent_issue(State(state), Json(req))
        .await
        .expect_err("expected issue registration error")
}

#[test]
fn issue_helpers_normalize_context_and_status() {
    assert_eq!(trim_optional(Some("  value  ")).as_deref(), Some("value"));
    assert_eq!(trim_optional(Some("   ")), None);
    assert_eq!(
        normalize_token(Some("High impact / Needs Branch"), "medium"),
        "high_impact_needs_branch"
    );
    assert_eq!(normalize_token(Some("..."), "medium"), "");
    assert_eq!(normalize_token(None, "medium"), "medium");

    let mut req = issue_request(None);
    req.source_context_type = Some("agent_conversation".to_string());
    req.source_context_id = Some(" conversation-1 ".to_string());
    assert_eq!(
        request_origin_conversation_id(&req).as_deref(),
        Some("conversation-1")
    );

    req.source_context_type = Some("review".to_string());
    req.source_context_id = Some(" review-task ".to_string());
    req.source_task_id = None;
    assert_eq!(request_source_task_id(&req).as_deref(), Some("review-task"));

    assert_eq!(
        validate_status(AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED).unwrap(),
        AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED
    );
    assert!(validate_status("waiting").is_err());
}

#[test]
fn followup_request_from_issue_builds_default_and_override_prompts() {
    let project_id = ProjectId::new();
    let origin = ChatConversation::new_project(project_id.clone());
    let issue = AgentConversationIssue::new(
        project_id,
        origin.id.clone(),
        Some("task-1".to_string()),
        Some("review".to_string()),
        Some("review-1".to_string()),
        Some("ralphx-execution-reviewer".to_string()),
        "plan_drift".to_string(),
        "high".to_string(),
        "followup_only".to_string(),
        "Investigate drift".to_string(),
        "Summary text".to_string(),
        Some("Evidence text".to_string()),
        Some("Recommendation text".to_string()),
        Some("drift:task-1".to_string()),
        None,
        None,
        true,
    );

    let default_request = followup_request_from_issue(&issue, None, None, None, None, None);
    assert_eq!(
        default_request.origin_conversation_id,
        Some(origin.id.as_str())
    );
    assert_eq!(default_request.source_task_id.as_deref(), Some("task-1"));
    assert_eq!(default_request.title, "Investigate drift");
    let prompt = default_request.initial_prompt.unwrap();
    assert!(prompt.contains("Summary text"));
    assert!(prompt.contains("Evidence:\nEvidence text"));
    assert!(prompt.contains("Recommended follow-up:\nRecommendation text"));

    let override_request = followup_request_from_issue(
        &issue,
        Some("codex".to_string()),
        Some("gpt-5.4".to_string()),
        None,
        Some("Custom title".to_string()),
        Some("Custom prompt".to_string()),
    );
    assert_eq!(override_request.title, "Custom title");
    assert_eq!(
        override_request.initial_prompt.as_deref(),
        Some("Custom prompt")
    );
    assert_eq!(override_request.provider_harness.as_deref(), Some("codex"));
    assert_eq!(override_request.model_override.as_deref(), Some("gpt-5.4"));
}

#[tokio::test]
async fn register_issue_validates_required_fields_and_origin_context() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut req = issue_request(None);
    req.title = " ".to_string();
    assert_eq!(
        register_issue_error(state.clone(), req).await.0,
        StatusCode::BAD_REQUEST
    );

    let mut req = issue_request(None);
    req.summary = " ".to_string();
    assert_eq!(
        register_issue_error(state.clone(), req).await.0,
        StatusCode::BAD_REQUEST
    );

    let mut req = issue_request(None);
    req.issue_kind = " ".to_string();
    assert_eq!(
        register_issue_error(state.clone(), req).await.0,
        StatusCode::BAD_REQUEST
    );

    let req = origin_only_issue_request(None);
    assert_eq!(
        register_issue_error(state.clone(), req).await.0,
        StatusCode::BAD_REQUEST
    );

    let task_origin = ChatConversation::new_task(crate::domain::entities::TaskId::new());
    app_state
        .chat_conversation_repo
        .create(task_origin.clone())
        .await
        .unwrap();
    let req = origin_only_issue_request(Some(task_origin.id.as_str()));
    assert_eq!(
        register_issue_error(state, req).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn register_issue_resolves_origin_from_source_task_workspace() {
    let app_state = Arc::new(AppState::new_test());
    let settings = ReviewSettings {
        auto_create_followup_agent_conversation: false,
        ..Default::default()
    };
    app_state
        .review_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let session_id = IdeationSessionId::from_string("session-issue");
    let task = seed_source_task(&app_state, &project_id, Some(session_id.clone())).await;
    let mut workspace = test_workspace(&origin, &project_id);
    workspace.linked_ideation_session_id = Some(session_id);
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let response = register_issue_ok(state, {
        let mut req = issue_request(None);
        req.source_task_id = Some(task.id.as_str().to_string());
        req
    })
    .await;

    assert_eq!(response.issue.conversation_id, origin.id.as_str());
    assert_eq!(
        response.issue.source_task_id.as_deref(),
        Some(task.id.as_str())
    );
    assert!(!response.auto_followup_created);
}

#[tokio::test]
async fn register_issue_rejects_invalid_source_task_origins() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;

    let task_without_session = seed_source_task(&app_state, &project_id, None).await;
    let no_session = register_issue_error(state.clone(), {
        let mut req = issue_request(None);
        req.source_task_id = Some(task_without_session.id.as_str().to_string());
        req
    })
    .await;
    assert_eq!(no_session.0, StatusCode::BAD_REQUEST);

    let session_id = IdeationSessionId::from_string("session-issue-no-workspace");
    let task_without_workspace =
        seed_source_task(&app_state, &project_id, Some(session_id.clone())).await;
    let no_workspace = register_issue_error(state.clone(), {
        let mut req = issue_request(None);
        req.source_task_id = Some(task_without_workspace.id.as_str().to_string());
        req
    })
    .await;
    assert_eq!(no_workspace.0, StatusCode::BAD_REQUEST);

    let orphan_session_id = IdeationSessionId::from_string("session-issue-orphan");
    let orphan_task =
        seed_source_task(&app_state, &project_id, Some(orphan_session_id.clone())).await;
    let orphan_conversation = ChatConversation::new_project(project_id.clone());
    let mut orphan_workspace = test_workspace(&orphan_conversation, &project_id);
    orphan_workspace.linked_ideation_session_id = Some(orphan_session_id);
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(orphan_workspace)
        .await
        .unwrap();
    let missing_conversation = register_issue_error(state.clone(), {
        let mut req = issue_request(None);
        req.source_task_id = Some(orphan_task.id.as_str().to_string());
        req
    })
    .await;
    assert_eq!(missing_conversation.0, StatusCode::NOT_FOUND);

    let other_project_id = ProjectId::new();
    let other_task = seed_source_task(&app_state, &other_project_id, None).await;
    let project_mismatch = register_issue_error(state, {
        let mut req = issue_request(Some(origin.id.as_str()));
        req.source_task_id = Some(other_task.id.as_str().to_string());
        req
    })
    .await;
    assert_eq!(project_mismatch.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_issue_saves_and_refreshes_existing_when_auto_followup_disabled() {
    let app_state = Arc::new(AppState::new_test());
    let settings = ReviewSettings {
        auto_create_followup_agent_conversation: false,
        ..Default::default()
    };
    app_state
        .review_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();
    let state = test_http_state(Arc::clone(&app_state));
    let (_project_id, origin) = seed_project_conversation(&app_state).await;

    let mut first_req = origin_only_issue_request(Some(origin.id.as_str()));
    first_req.auto_followup_eligible = true;
    let first = register_issue_ok(state.clone(), first_req).await;
    assert!(!first.auto_followup_created);
    assert!(first.followup.is_none());
    assert_eq!(first.issue.issue_kind, "plan_drift");
    assert_eq!(first.issue.severity, "high_impact");
    assert_eq!(first.issue.blocking_scope, "follow_up_only");

    let mut second_req = origin_only_issue_request(Some(origin.id.as_str()));
    second_req.auto_followup_eligible = true;
    second_req.title = "Updated drift title".to_string();
    second_req.evidence = Some("new evidence".to_string());
    let second = register_issue_ok(state, second_req).await;

    assert_eq!(second.issue.id, first.issue.id);
    assert_eq!(second.issue.title, "Updated drift title");
    assert_eq!(second.issue.evidence.as_deref(), Some("new evidence"));
    assert_eq!(second.issue.status, AGENT_CONVERSATION_ISSUE_STATUS_OPEN);
    assert_eq!(first.dedupe_result, AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED);
    assert_eq!(
        second.dedupe_result,
        AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED
    );
    assert_eq!(second.occurrence_count, Some(2));
}

#[tokio::test]
async fn register_issue_exact_canonical_duplicate_appends_occurrence() {
    let app_state = Arc::new(AppState::new_test());
    let settings = ReviewSettings {
        auto_create_followup_agent_conversation: false,
        ..Default::default()
    };
    app_state
        .review_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();
    let state = test_http_state(Arc::clone(&app_state));
    let (_project_id, origin) = seed_project_conversation(&app_state).await;

    let mut first_req = origin_only_issue_request(Some(origin.id.as_str()));
    first_req.title =
        "Frontend validation analysis uses repo-root setup for frontend package".to_string();
    first_req.summary =
        "The frontend validation command could not find frontend/node_modules.".to_string();
    first_req.evidence = Some("pnpm exec tsc missing frontend/node_modules".to_string());
    first_req.blocker_fingerprint = Some("frontend-validation-cwd-node-modules-setup".to_string());
    let first = register_issue_ok(state.clone(), first_req).await;

    let mut second_req = origin_only_issue_request(Some(origin.id.as_str()));
    second_req.title = "Merge hook cannot find tsc".to_string();
    second_req.summary = "The merge hook failed because tsc was not found on PATH.".to_string();
    second_req.evidence = Some("sh: tsc: command not found".to_string());
    second_req.blocker_fingerprint = Some("merge-hook-env:task-b:tsc-not-found".to_string());
    let second = register_issue_ok(state, second_req).await;

    assert_eq!(second.issue.id, first.issue.id);
    assert_eq!(
        second.dedupe_result,
        AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED
    );
    assert_eq!(
        second.canonical_fingerprint.as_deref(),
        Some("v1:setup:project:frontend-package:missing-frontend-dependency")
    );
    assert_eq!(second.occurrence_count, Some(2));
    assert_eq!(second.issue.occurrences.len(), 2);
}

fn candidate_identity() -> AgentConversationIssueCanonicalIdentity {
    AgentConversationIssueCanonicalIdentity {
        fingerprint: "v1:setup:project:frontend-package:other-setup-failure".to_string(),
        scope_kind: "project".to_string(),
        scope_subject: "frontend-package".to_string(),
        family: "setup".to_string(),
        candidate_match_eligible: true,
    }
}

async fn seed_frontend_setup_candidate(
    app_state: &AppState,
    project_id: ProjectId,
    origin: &ChatConversation,
) -> AgentConversationIssue {
    let mut issue = AgentConversationIssue::new(
        project_id,
        origin.id.clone(),
        None,
        Some("review".to_string()),
        Some("review-1".to_string()),
        Some("ralphx-execution-reviewer".to_string()),
        "plan_drift".to_string(),
        "medium".to_string(),
        "followup_only".to_string(),
        "Existing frontend setup issue".to_string(),
        "A related but not exact frontend setup issue already exists.".to_string(),
        None,
        None,
        Some("frontend-setup:other".to_string()),
        None,
        None,
        false,
    );
    issue.apply_canonical_identity(&candidate_identity());
    app_state
        .agent_conversation_issue_repo
        .save(&issue)
        .await
        .unwrap()
}

#[tokio::test]
async fn register_issue_requires_disambiguation_for_canonical_candidates() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let candidate = seed_frontend_setup_candidate(&app_state, project_id, &origin).await;

    let mut req = origin_only_issue_request(Some(origin.id.as_str()));
    req.title = "Merge hook cannot find tsc".to_string();
    req.summary = "The merge hook failed because tsc was not found on PATH.".to_string();
    req.evidence = Some("sh: tsc: command not found".to_string());
    req.blocker_fingerprint = Some("merge-hook-env:task-b:tsc-not-found".to_string());

    let error = register_issue_error(state.clone(), req).await;
    assert_eq!(error.0, StatusCode::CONFLICT);
    assert_eq!(
        error.1 .0.get("error").and_then(|value| value.as_str()),
        Some("needs_issue_disambiguation")
    );
    assert_eq!(
        error.1 .0["candidate_issues"][0]["id"].as_str(),
        Some(candidate.id.as_str())
    );

    let list = list_agent_conversation_issues(
        State(state),
        Json(ListAgentConversationIssuesRequest {
            conversation_id: origin.id.as_str(),
            include_resolved: false,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(list.issues.len(), 1);
}

#[tokio::test]
async fn register_issue_confirm_new_requires_reason_and_token() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    seed_frontend_setup_candidate(&app_state, project_id, &origin).await;

    let mut req = origin_only_issue_request(Some(origin.id.as_str()));
    req.title = "Merge hook cannot find tsc".to_string();
    req.summary = "The merge hook failed because tsc was not found on PATH.".to_string();
    req.evidence = Some("sh: tsc: command not found".to_string());
    req.blocker_fingerprint = Some("merge-hook-env:task-b:tsc-not-found".to_string());

    let conflict = register_issue_error(state.clone(), req.clone()).await;
    let token = conflict.1 .0["issue_check_token"]
        .as_str()
        .expect("candidate conflict should return token")
        .to_string();

    let mut missing_reason = req.clone();
    missing_reason.confirm_new = true;
    missing_reason.issue_check_token = Some(token.clone());
    assert_eq!(
        register_issue_error(state.clone(), missing_reason).await.0,
        StatusCode::BAD_REQUEST
    );

    req.confirm_new = true;
    req.new_issue_reason = Some("This hook failure is a separate PATH issue.".to_string());
    req.issue_check_token = Some(token);
    let response = register_issue_ok(state.clone(), req).await;
    assert_eq!(
        response.dedupe_result,
        AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW
    );

    let list = list_agent_conversation_issues(
        State(state),
        Json(ListAgentConversationIssuesRequest {
            conversation_id: origin.id.as_str(),
            include_resolved: false,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(list.issues.len(), 2);
}

#[tokio::test]
async fn register_issue_attach_to_issue_id_appends_candidate_occurrence() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let candidate = seed_frontend_setup_candidate(&app_state, project_id, &origin).await;

    let mut req = origin_only_issue_request(Some(origin.id.as_str()));
    req.title = "Merge hook cannot find tsc".to_string();
    req.summary = "The merge hook failed because tsc was not found on PATH.".to_string();
    req.evidence = Some("sh: tsc: command not found".to_string());
    req.blocker_fingerprint = Some("merge-hook-env:task-b:tsc-not-found".to_string());
    req.attach_to_issue_id = Some(candidate.id.clone());

    let response = register_issue_ok(state, req).await;
    assert_eq!(response.issue.id, candidate.id);
    assert_eq!(
        response.dedupe_result,
        AGENT_CONVERSATION_ISSUE_DEDUPE_CANDIDATE_ATTACHED
    );
    assert_eq!(response.occurrence_count, Some(1));
}

#[tokio::test]
async fn register_issue_attempts_auto_followup_when_enabled() {
    let app_state = Arc::new(AppState::new_test());
    app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            auto_create_followup_agent_conversation: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let state = test_http_state(Arc::clone(&app_state));
    let (_project_id, origin) = seed_project_conversation(&app_state).await;

    let mut req = origin_only_issue_request(Some(origin.id.as_str()));
    req.auto_followup_eligible = true;
    let error = register_issue_error(state.clone(), req).await;
    assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);

    let list = list_agent_conversation_issues(
        State(state),
        Json(ListAgentConversationIssuesRequest {
            conversation_id: origin.id.as_str(),
            include_resolved: false,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(list.issues.len(), 1);
    assert!(list.issues[0].linked_followup_conversation_id.is_none());
}

#[tokio::test]
async fn list_and_status_update_filter_resolved_and_dismissed_issues() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let open_issue = AgentConversationIssue::new(
        project_id.clone(),
        origin.id.clone(),
        None,
        None,
        None,
        None,
        "plan_drift".to_string(),
        "medium".to_string(),
        "none".to_string(),
        "Open issue".to_string(),
        "Open summary".to_string(),
        None,
        None,
        None,
        None,
        None,
        false,
    );
    let resolved_issue = AgentConversationIssue::new(
        project_id,
        origin.id.clone(),
        None,
        None,
        None,
        None,
        "decision".to_string(),
        "low".to_string(),
        "none".to_string(),
        "Resolved issue".to_string(),
        "Resolved summary".to_string(),
        None,
        None,
        None,
        None,
        None,
        false,
    );
    let open_issue = app_state
        .agent_conversation_issue_repo
        .save(&open_issue)
        .await
        .unwrap();
    let resolved_issue = app_state
        .agent_conversation_issue_repo
        .save(&resolved_issue)
        .await
        .unwrap();

    let updated = update_agent_conversation_issue_status(
        State(state.clone()),
        Json(UpdateAgentConversationIssueStatusRequest {
            issue_id: resolved_issue.id.clone(),
            status: AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED.to_string(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(
        updated.issue.status,
        AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED
    );
    assert!(updated.issue.resolved_at.is_some());

    let unresolved = list_agent_conversation_issues(
        State(state.clone()),
        Json(ListAgentConversationIssuesRequest {
            conversation_id: origin.id.as_str(),
            include_resolved: false,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(unresolved.issues.len(), 1);
    assert_eq!(unresolved.issues[0].id, open_issue.id);

    let all = list_agent_conversation_issues(
        State(state.clone()),
        Json(ListAgentConversationIssuesRequest {
            conversation_id: origin.id.as_str(),
            include_resolved: true,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(all.issues.len(), 2);

    let invalid = update_agent_conversation_issue_status(
        State(state.clone()),
        Json(UpdateAgentConversationIssueStatusRequest {
            issue_id: open_issue.id.clone(),
            status: "waiting".to_string(),
        }),
    )
    .await
    .expect_err("invalid status should be rejected");
    assert_eq!(invalid.0, StatusCode::BAD_REQUEST);

    let missing = update_agent_conversation_issue_status(
        State(state),
        Json(UpdateAgentConversationIssueStatusRequest {
            issue_id: "missing".to_string(),
            status: AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED.to_string(),
        }),
    )
    .await
    .expect_err("missing issue should be rejected");
    assert_eq!(missing.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn convert_issue_followup_reuses_existing_conversation_and_links_issue() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let task = app_state
        .task_repo
        .create(Task::new(project_id.clone(), "Source task".to_string()))
        .await
        .unwrap();
    let source_task_id = task.id.as_str().to_string();
    let issue = AgentConversationIssue::new(
        project_id.clone(),
        origin.id.clone(),
        Some(source_task_id.clone()),
        Some("review".to_string()),
        Some("review-1".to_string()),
        Some("ralphx-execution-reviewer".to_string()),
        "plan_drift".to_string(),
        "high".to_string(),
        "followup_only".to_string(),
        "Investigate drift".to_string(),
        "The task uncovered unrelated work.".to_string(),
        None,
        Some("Open a follow-up".to_string()),
        Some("drift:task-1".to_string()),
        None,
        None,
        true,
    );
    let issue = app_state
        .agent_conversation_issue_repo
        .save(&issue)
        .await
        .unwrap();
    let followup_conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(followup_conversation.clone())
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(test_workspace(&followup_conversation, &project_id))
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(
            &followup_conversation.id,
            AgentWorkspaceFollowupProvenance {
                origin_conversation_id: origin.id.clone(),
                source_task_id: Some(source_task_id),
                source_context_type: Some("review".to_string()),
                source_context_id: Some("review-1".to_string()),
                source_agent_name: Some("ralphx-execution-reviewer".to_string()),
                spawn_reason: Some("plan_drift".to_string()),
                blocker_fingerprint: Some("drift:task-1".to_string()),
            },
        )
        .await
        .unwrap();

    let response = convert_agent_conversation_issue_followup(
        State(state),
        Json(ConvertAgentConversationIssueFollowupRequest {
            issue_id: issue.id,
            title: Some("  ".to_string()),
            initial_prompt: Some("  ".to_string()),
            provider_harness: None,
            model_override: None,
            logical_effort: None,
        }),
    )
    .await
    .unwrap()
    .0;

    assert!(response.followup.reused_existing);
    assert_eq!(
        response.followup.conversation.id,
        followup_conversation.id.as_str()
    );
    assert_eq!(
        response.issue.linked_followup_conversation_id.as_deref(),
        Some(followup_conversation.id.as_str().as_str())
    );
}
