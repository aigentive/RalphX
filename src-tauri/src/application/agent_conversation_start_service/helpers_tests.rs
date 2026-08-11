//! Unit tests for the pure `agent_conversation_start_service` helpers.
//!
//! These exercise the parsing/normalization helpers and the runtime-selection
//! logic without spawning agents or touching git, so they stay fast and
//! deterministic. The integration-only `start()` orchestration lives in the
//! parent module and is covered by higher-level flows.

use std::sync::Arc;

use super::*;
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, Project,
};
use crate::domain::services::PrSearchResult;
use crate::tests::mock_github_service::MockGithubService;

// ── parse_agent_workspace_mode ───────────────────────────────────────────────

#[test]
fn parse_mode_defaults_to_edit_for_none_and_blank() {
    assert_eq!(
        parse_agent_workspace_mode(None).unwrap(),
        AgentConversationWorkspaceMode::Edit
    );
    assert_eq!(
        parse_agent_workspace_mode(Some("   ")).unwrap(),
        AgentConversationWorkspaceMode::Edit
    );
    // An empty (non-whitespace) string also falls back to the "edit" default.
    assert_eq!(
        parse_agent_workspace_mode(Some("")).unwrap(),
        AgentConversationWorkspaceMode::Edit
    );
}

#[test]
fn parse_mode_trims_and_parses_known_modes() {
    assert_eq!(
        parse_agent_workspace_mode(Some("  plan ")).unwrap(),
        AgentConversationWorkspaceMode::Plan
    );
    assert_eq!(
        parse_agent_workspace_mode(Some("chat")).unwrap(),
        AgentConversationWorkspaceMode::Chat
    );
    assert_eq!(
        parse_agent_workspace_mode(Some("review_pr")).unwrap(),
        AgentConversationWorkspaceMode::ReviewPr
    );
}

#[test]
fn parse_mode_rejects_unknown_value() {
    let error = parse_agent_workspace_mode(Some("nonsense")).unwrap_err();
    assert!(
        error.contains("nonsense"),
        "error should name the bad value: {error}"
    );
}

// ── parse_agent_workspace_base_kind ──────────────────────────────────────────

#[test]
fn parse_base_kind_is_none_for_absent_or_blank() {
    assert_eq!(parse_agent_workspace_base_kind(None).unwrap(), None);
    assert_eq!(parse_agent_workspace_base_kind(Some("   ")).unwrap(), None);
    assert_eq!(parse_agent_workspace_base_kind(Some("")).unwrap(), None);
}

#[test]
fn parse_base_kind_trims_and_parses_known_kinds() {
    assert_eq!(
        parse_agent_workspace_base_kind(Some(" local_branch ")).unwrap(),
        Some(IdeationAnalysisBaseRefKind::LocalBranch)
    );
    assert_eq!(
        parse_agent_workspace_base_kind(Some("project_default")).unwrap(),
        Some(IdeationAnalysisBaseRefKind::ProjectDefault)
    );
}

#[test]
fn parse_base_kind_rejects_unknown_value() {
    let error = parse_agent_workspace_base_kind(Some("weird")).unwrap_err();
    assert!(
        error.contains("weird"),
        "error should name the bad value: {error}"
    );
}

// ── parse_agent_workspace_branch_mode ───────────────────────────────────────

#[test]
fn parse_branch_mode_defaults_and_rejects_unknown_value() {
    assert_eq!(
        AgentConversationWorkspaceBranchMode::default(),
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_eq!(parse_agent_workspace_branch_mode(None).unwrap(), None);
    assert_eq!(
        parse_agent_workspace_branch_mode(Some("   ")).unwrap(),
        None
    );
    assert_eq!(
        parse_agent_workspace_branch_mode(Some(" linked ")).unwrap(),
        Some(AgentConversationWorkspaceBranchMode::Linked)
    );
    assert_eq!(
        parse_agent_workspace_branch_mode(Some("isolated")).unwrap(),
        Some(AgentConversationWorkspaceBranchMode::Isolated)
    );

    let error = parse_agent_workspace_branch_mode(Some("shared")).unwrap_err();
    assert!(
        error.contains("unknown agent workspace branch mode"),
        "got: {error}"
    );
}

// ── trim_optional_input ──────────────────────────────────────────────────────

#[test]
fn trim_optional_input_drops_none_and_blank() {
    assert_eq!(trim_optional_input(None), None);
    assert_eq!(trim_optional_input(Some("   ".to_string())), None);
    assert_eq!(trim_optional_input(Some(String::new())), None);
}

#[test]
fn trim_optional_input_trims_surrounding_whitespace() {
    assert_eq!(
        trim_optional_input(Some("  hello world  ".to_string())),
        Some("hello world".to_string())
    );
}

// ── normalize_agent_workspace_source_pull_request ────────────────────────────

fn pr_input(number: i64, head_ref_name: &str) -> AgentWorkspaceSourcePullRequestInput {
    AgentWorkspaceSourcePullRequestInput {
        number,
        url: Some("  https://example/pr/1  ".to_string()),
        title: Some("  Title  ".to_string()),
        head_ref_name: head_ref_name.to_string(),
        base_ref_name: Some("  main  ".to_string()),
        head_ref_oid: Some("  abc123  ".to_string()),
    }
}

#[test]
fn normalize_source_pr_is_none_when_input_absent() {
    let result = normalize_agent_workspace_source_pull_request(
        None,
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("feature"),
    )
    .unwrap();
    assert!(result.is_none());
}

#[test]
fn normalize_source_pr_rejects_non_positive_number() {
    for bad in [0, -5] {
        let error = normalize_agent_workspace_source_pull_request(
            Some(pr_input(bad, "feature")),
            Some(IdeationAnalysisBaseRefKind::LocalBranch),
            None,
        )
        .unwrap_err();
        assert!(error.contains("number must be positive"), "got: {error}");
    }
}

#[test]
fn normalize_source_pr_requires_local_branch_base_kind() {
    let error = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "feature")),
        Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        None,
    )
    .unwrap_err();
    assert!(
        error.contains("requires a local_branch base ref"),
        "got: {error}"
    );

    // None base kind is also rejected.
    let error =
        normalize_agent_workspace_source_pull_request(Some(pr_input(7, "feature")), None, None)
            .unwrap_err();
    assert!(
        error.contains("requires a local_branch base ref"),
        "got: {error}"
    );
}

#[test]
fn normalize_source_pr_requires_non_empty_head_ref() {
    let error = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "   ")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        None,
    )
    .unwrap_err();
    assert!(error.contains("head branch is required"), "got: {error}");
}

#[test]
fn normalize_source_pr_rejects_base_ref_mismatch() {
    let error = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "feature")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("  other-branch  "),
    )
    .unwrap_err();
    assert!(
        error.contains("must match the selected base ref"),
        "got: {error}"
    );
}

#[test]
fn normalize_source_pr_accepts_matching_base_ref_and_trims_fields() {
    let pr = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "  feature  ")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("  feature  "),
    )
    .unwrap()
    .expect("a valid PR input yields Some");

    assert_eq!(pr.number, 7);
    assert_eq!(pr.head_ref_name, "feature");
    assert_eq!(pr.url.as_deref(), Some("https://example/pr/1"));
    assert_eq!(pr.title.as_deref(), Some("Title"));
    assert_eq!(pr.base_ref_name.as_deref(), Some("main"));
    assert_eq!(pr.head_ref_oid.as_deref(), Some("abc123"));
}

#[test]
fn normalize_source_pr_accepts_blank_base_ref_without_mismatch() {
    // A blank base_ref is filtered out, so no mismatch check runs.
    let pr = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "feature")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("   "),
    )
    .unwrap()
    .expect("blank base ref does not trigger mismatch");
    assert_eq!(pr.head_ref_name, "feature");
}

#[test]
fn normalize_source_pr_drops_blank_optional_fields() {
    let input = AgentWorkspaceSourcePullRequestInput {
        number: 3,
        url: Some("   ".to_string()),
        title: None,
        head_ref_name: "feature".to_string(),
        base_ref_name: Some(String::new()),
        head_ref_oid: None,
    };
    let pr = normalize_agent_workspace_source_pull_request(
        Some(input),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        None,
    )
    .unwrap()
    .expect("valid input");
    assert!(pr.url.is_none());
    assert!(pr.title.is_none());
    assert!(pr.base_ref_name.is_none());
    assert!(pr.head_ref_oid.is_none());
}

// ── ticket start base fallback helpers ───────────────────────────────────────

fn integration_ref(
    provider: &str,
    kind: &str,
    id: &str,
    key: Option<&str>,
) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: provider.to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
        key: key.map(str::to_string),
        title: None,
        url: None,
        summary_excerpt: None,
        include_transcript: None,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
    }
}

#[test]
fn first_ticket_start_base_reference_prefers_ticket_integrations() {
    let references = vec![
        integration_ref("atlassian", "confluence", "space-1", None),
        integration_ref("atlassian", "jira", "10001", Some("RX-24")),
        integration_ref("linear", "linear", "lin-1", Some("ENG-5")),
    ];

    let ticket = first_ticket_start_base_reference(&references).expect("jira reference");

    assert_eq!(ticket.provider, "jira");
    assert_eq!(ticket.issue_key, "RX-24");
}

#[test]
fn first_ticket_start_base_reference_supports_linear_clickup_and_id_fallback() {
    let linear =
        first_ticket_start_base_reference(&[integration_ref("linear", "linear", "lin-1", None)])
            .expect("linear reference");
    assert_eq!(linear.provider, "linear");
    assert_eq!(linear.issue_key, "lin-1");

    let clickup = first_ticket_start_base_reference(&[integration_ref(
        "clickup",
        "clickup",
        "task-1",
        Some("CU-1"),
    )])
    .expect("clickup reference");
    assert_eq!(clickup.provider, "clickup");
    assert_eq!(clickup.issue_key, "CU-1");
}

#[test]
fn first_ticket_start_base_reference_ignores_unsupported_and_blank_ticket_refs() {
    assert!(first_ticket_start_base_reference(&[integration_ref(
        "atlassian",
        "confluence",
        "SPACE",
        None,
    )])
    .is_none());

    assert!(first_ticket_start_base_reference(&[integration_ref(
        "linear",
        "linear",
        "   ",
        Some("   "),
    )])
    .is_none());
}

#[test]
fn base_selection_allows_ticket_canonical_branch_only_for_default_without_pr() {
    assert!(base_selection_allows_ticket_canonical_branch(None, None));
    assert!(base_selection_allows_ticket_canonical_branch(
        Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        None,
    ));

    let source = AgentWorkspaceSourcePullRequest {
        number: 42,
        url: None,
        title: None,
        head_ref_name: "feature/pr-head".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: None,
    };
    assert!(!base_selection_allows_ticket_canonical_branch(
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        None,
    ));
    assert!(!base_selection_allows_ticket_canonical_branch(
        Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        Some(&source),
    ));
}

#[test]
fn apply_ticket_canonical_branch_base_selection_sets_local_branch_base() {
    let mut kind = Some(IdeationAnalysisBaseRefKind::ProjectDefault);
    let mut base_ref = Some("main".to_string());
    let mut display_name = Some("Project default (main)".to_string());

    apply_ticket_canonical_branch_base_selection(
        &mut kind,
        &mut base_ref,
        &mut display_name,
        "RX-24",
        "ralphx/ticket/jira-rx-24",
    );

    assert_eq!(kind, Some(IdeationAnalysisBaseRefKind::LocalBranch));
    assert_eq!(base_ref.as_deref(), Some("ralphx/ticket/jira-rx-24"));
    assert_eq!(
        display_name.as_deref(),
        Some("Ticket RX-24 (ralphx/ticket/jira-rx-24)")
    );
}

// ── agent_mode_requires_workspace / agent_mode_should_create_workspace ────────

#[test]
fn requires_workspace_is_true_for_non_chat_modes() {
    use AgentConversationWorkspaceMode::*;
    assert!(agent_mode_requires_workspace(Edit));
    assert!(agent_mode_requires_workspace(Plan));
    assert!(agent_mode_requires_workspace(Ideation));
    assert!(agent_mode_requires_workspace(ReviewPr));
    assert!(!agent_mode_requires_workspace(Chat));
}

#[test]
fn should_create_workspace_covers_chat_with_source_pr() {
    use AgentConversationWorkspaceMode::*;
    // Non-chat modes always create a workspace regardless of PR.
    assert!(agent_mode_should_create_workspace(Edit, None));

    // Chat without a source PR does not create a workspace.
    assert!(!agent_mode_should_create_workspace(Chat, None));

    // Chat WITH a source PR does create a workspace.
    let source = AgentWorkspaceSourcePullRequest {
        number: 1,
        url: None,
        title: None,
        head_ref_name: "feature".to_string(),
        base_ref_name: None,
        head_ref_oid: None,
    };
    assert!(agent_mode_should_create_workspace(Chat, Some(&source)));
}

// ── linked branch availability / PR hydration ────────────────────────────────

#[tokio::test]
async fn linked_branch_availability_detects_conflicts_and_exempts_current_conversation() {
    let state = AppState::new_test();
    let project = Project::new("Demo".to_string(), "/tmp/demo".to_string());
    let existing = workspace_for_mode(&project, AgentConversationWorkspaceMode::Edit);
    state
        .agent_conversation_workspace_repo
        .create_or_update(existing.clone())
        .await
        .expect("seed existing linked workspace");

    ensure_linked_branch_workspace_available(
        &state,
        &project.id,
        None,
        Some(AgentConversationWorkspaceBranchMode::Isolated),
        Some("feature/branch"),
        None,
    )
    .await
    .expect("isolated mode does not reserve the selected branch");

    ensure_linked_branch_workspace_available(
        &state,
        &project.id,
        None,
        Some(AgentConversationWorkspaceBranchMode::Linked),
        Some("   "),
        None,
    )
    .await
    .expect("blank linked branch has nothing to reserve");

    let conflict = ensure_linked_branch_workspace_available(
        &state,
        &project.id,
        None,
        Some(AgentConversationWorkspaceBranchMode::Linked),
        Some(" feature/branch "),
        None,
    )
    .await
    .expect_err("another active workspace already owns this linked branch");
    assert!(conflict.contains("feature/branch"), "got: {conflict}");
    assert!(
        conflict.contains(&existing.conversation_id.as_str()),
        "got: {conflict}"
    );

    ensure_linked_branch_workspace_available(
        &state,
        &project.id,
        Some(&existing.conversation_id),
        Some(AgentConversationWorkspaceBranchMode::Linked),
        Some("feature/branch"),
        None,
    )
    .await
    .expect("the current conversation can re-check its own linked branch");

    let source = AgentWorkspaceSourcePullRequest {
        number: 42,
        url: None,
        title: None,
        head_ref_name: "feature/branch".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: None,
    };
    let conflict = ensure_linked_branch_workspace_available(
        &state,
        &project.id,
        None,
        Some(AgentConversationWorkspaceBranchMode::Linked),
        Some("main"),
        Some(&source),
    )
    .await
    .expect_err("PR-backed linked workspaces reserve the PR head branch");
    assert!(conflict.contains("feature/branch"), "got: {conflict}");
}

fn pr_search_result(
    number: i64,
    head_ref_name: &str,
    base_ref_name: &str,
    is_cross_repository: bool,
) -> PrSearchResult {
    PrSearchResult {
        number,
        title: format!("PR {number}"),
        url: format!("https://github.com/acme/demo/pull/{number}"),
        head_ref_name: head_ref_name.to_string(),
        head_ref_oid: Some(format!("sha-{number}")),
        base_ref_name: base_ref_name.to_string(),
        is_draft: false,
        updated_at: None,
        author_login: Some("dev".to_string()),
        assignee_logins: Vec::new(),
        review_decision: None,
        latest_review_author_logins: Vec::new(),
        review_request_logins: Vec::new(),
        is_cross_repository,
    }
}

#[tokio::test]
async fn linked_branch_pr_hydration_uses_matching_same_repo_pull_request() {
    let github = Arc::new(MockGithubService::new());
    github.will_return_pull_request_search(vec![
        pr_search_result(7, "feature/shared", "main", true),
        pr_search_result(8, "other", "main", false),
        pr_search_result(9, "feature/shared", "release", false),
    ]);

    let mut state = AppState::new_test();
    state.github_service = Some(github.clone());
    let project = Project::new("Demo".to_string(), "/tmp/demo".to_string());

    let hydrated = hydrate_linked_branch_source_pull_request(
        &state,
        &project,
        Some(AgentConversationWorkspaceBranchMode::Linked),
        Some(" feature/shared "),
        None,
    )
    .await
    .expect("PR hydration should not fail")
    .expect("matching same-repository PR should be hydrated");

    assert_eq!(hydrated.number, 9);
    assert_eq!(hydrated.head_ref_name, "feature/shared");
    assert_eq!(hydrated.base_ref_name.as_deref(), Some("release"));
    assert_eq!(hydrated.head_ref_oid.as_deref(), Some("sha-9"));
    assert_eq!(
        hydrated.url.as_deref(),
        Some("https://github.com/acme/demo/pull/9")
    );
    assert_eq!(hydrated.title.as_deref(), Some("PR 9"));

    let state = github.state();
    assert_eq!(state.search_pull_requests_calls, 1);
    assert_eq!(
        state.last_search_pull_requests_args,
        Some((Some("feature/shared".to_string()), 20))
    );
}

// ── normalized_effort_for_supported ──────────────────────────────────────────

#[test]
fn normalized_effort_keeps_supported_request() {
    let supported = [LogicalEffort::Low, LogicalEffort::High];
    assert_eq!(
        normalized_effort_for_supported(Some(LogicalEffort::High), &supported, LogicalEffort::Low),
        LogicalEffort::High
    );
}

#[test]
fn normalized_effort_falls_back_when_unsupported_or_absent() {
    let supported = [LogicalEffort::Low, LogicalEffort::Medium];
    // Requested but unsupported → default.
    assert_eq!(
        normalized_effort_for_supported(
            Some(LogicalEffort::Max),
            &supported,
            LogicalEffort::Medium
        ),
        LogicalEffort::Medium
    );
    // Absent → default.
    assert_eq!(
        normalized_effort_for_supported(None, &supported, LogicalEffort::Low),
        LogicalEffort::Low
    );
}

// ── normalize_agent_runtime_selection ────────────────────────────────────────

#[tokio::test]
async fn runtime_selection_passes_through_overrides_without_provider() {
    let state = AppState::new_test();
    let (model, effort) = normalize_agent_runtime_selection(
        &state,
        None,
        Some("custom-model".to_string()),
        Some(LogicalEffort::High),
    )
    .await
    .unwrap();
    assert_eq!(model.as_deref(), Some("custom-model"));
    assert_eq!(effort, Some(LogicalEffort::High));
}

#[tokio::test]
async fn runtime_selection_uses_known_model_supported_effort() {
    let state = AppState::new_test();
    // "opus" supports XHigh, so the requested XHigh effort survives.
    let (model, effort) = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Claude),
        Some("opus".to_string()),
        Some(LogicalEffort::XHigh),
    )
    .await
    .unwrap();
    assert_eq!(model.as_deref(), Some("opus"));
    assert_eq!(effort, Some(LogicalEffort::XHigh));
}

#[tokio::test]
async fn runtime_selection_clamps_unsupported_effort_to_model_default() {
    let state = AppState::new_test();
    // "sonnet" does NOT support XHigh, so it falls back to sonnet's default (Medium).
    let (model, effort) = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Claude),
        Some("sonnet".to_string()),
        Some(LogicalEffort::XHigh),
    )
    .await
    .unwrap();
    assert_eq!(model.as_deref(), Some("sonnet"));
    assert_eq!(effort, Some(LogicalEffort::Medium));
}

#[tokio::test]
async fn runtime_selection_unknown_model_falls_back_to_provider_defaults() {
    let state = AppState::new_test();
    // Unknown model id keeps the model override but resolves effort from the
    // provider-wide defaults. Claude provider-default efforts are Low/Medium/High,
    // and the requested High is supported.
    let (model, effort) = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Claude),
        Some("ghost-model".to_string()),
        Some(LogicalEffort::High),
    )
    .await
    .unwrap();
    assert_eq!(model.as_deref(), Some("ghost-model"));
    assert_eq!(effort, Some(LogicalEffort::High));
}

#[tokio::test]
async fn runtime_selection_unknown_model_clamps_unsupported_effort_to_provider_default() {
    let state = AppState::new_test();
    // Claude provider defaults do NOT include Max, so it clamps to the provider
    // default effort (Medium for Claude).
    let (model, effort) = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Claude),
        Some("ghost-model".to_string()),
        Some(LogicalEffort::Max),
    )
    .await
    .unwrap();
    assert_eq!(model.as_deref(), Some("ghost-model"));
    assert_eq!(effort, Some(LogicalEffort::Medium));
}

#[tokio::test]
async fn runtime_selection_no_model_uses_default_model_supported_effort() {
    let state = AppState::new_test();
    // With no model override, the provider's default model (sonnet) is used to
    // resolve the effort. Sonnet supports High, so the requested High survives,
    // and no model id is returned.
    let (model, effort) = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Claude),
        None,
        Some(LogicalEffort::High),
    )
    .await
    .unwrap();
    assert_eq!(model, None);
    assert_eq!(effort, Some(LogicalEffort::High));
}

#[tokio::test]
async fn runtime_selection_no_model_no_effort_uses_default_model_default_effort() {
    let state = AppState::new_test();
    // No model and no effort → default model's default effort (sonnet → Medium).
    let (model, effort) =
        normalize_agent_runtime_selection(&state, Some(AgentHarnessKind::Claude), None, None)
            .await
            .unwrap();
    assert_eq!(model, None);
    assert_eq!(effort, Some(LogicalEffort::Medium));
}

// ── logging / progress emission ──────────────────────────────────────────────

#[test]
fn log_phase_does_not_panic_with_and_without_conversation_id() {
    let started = std::time::Instant::now();
    log_start_agent_conversation_phase("project-1", None, "setup", started);
    let conversation_id = ChatConversationId::new();
    log_start_agent_conversation_phase("project-1", Some(&conversation_id), "spawn", started);
}

#[test]
fn emit_progress_does_not_panic_on_mock_app_handle() {
    let app = crate::testing::create_mock_app_handle();
    let conversation_id = ChatConversationId::new();
    // The emit is best-effort (`let _ = ...`); this just exercises the payload
    // construction and emit path without asserting delivery.
    emit_start_agent_conversation_progress(
        &app,
        "project-1",
        &conversation_id,
        "stage-1",
        "Preparing workspace",
    );
}

// ── ensure_plan_workspace_planning_session_link (early returns) ───────────────

fn workspace_for_mode(
    project: &Project,
    mode: AgentConversationWorkspaceMode,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::new(),
        project.id.clone(),
        mode,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "feature/branch".to_string(),
        "/tmp/worktree".to_string(),
    )
}

#[tokio::test]
async fn ensure_plan_link_is_noop_for_non_plan_mode() {
    let state = AppState::new_test();
    let project = Project::new("Demo".to_string(), "/tmp/demo".to_string());
    let mut workspace = workspace_for_mode(&project, AgentConversationWorkspaceMode::Edit);

    let created = ensure_plan_workspace_planning_session_link(&state, &project, &mut workspace)
        .await
        .unwrap();

    // Edit mode never establishes a planning-session link.
    assert!(!created);
    assert!(workspace.linked_ideation_session_id.is_none());
}

#[tokio::test]
async fn ensure_plan_link_is_noop_when_already_linked_to_planning_session() {
    let state = AppState::new_test();
    let project = Project::new("Demo".to_string(), "/tmp/demo".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");

    // Seed a Planning-flow ideation session and link the workspace to it.
    let session = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .build();
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("seed ideation session");

    let mut workspace = workspace_for_mode(&project, AgentConversationWorkspaceMode::Plan);
    workspace.linked_ideation_session_id = Some(session.id.clone());

    let created = ensure_plan_workspace_planning_session_link(&state, &project, &mut workspace)
        .await
        .unwrap();

    // Already linked to a planning session → nothing new is created and the link
    // is preserved.
    assert!(!created);
    assert_eq!(workspace.linked_ideation_session_id, Some(session.id));
}

#[tokio::test]
async fn ensure_plan_link_surfaces_error_when_analysis_preparation_fails() {
    let state = AppState::new_test();
    // Point the project at a non-repository directory so the workspace-path
    // resolution / analysis preparation step fails. A Plan-mode workspace whose
    // linked session is missing passes the planning short-circuit and reaches the
    // analysis-preparation step, which must surface a typed error string rather
    // than silently succeeding.
    let project = Project::new("Demo".to_string(), "/nonexistent/ralphx-demo".to_string());
    let mut workspace = workspace_for_mode(&project, AgentConversationWorkspaceMode::Plan);
    workspace.linked_ideation_session_id = Some(crate::domain::entities::IdeationSessionId::new());

    let result =
        ensure_plan_workspace_planning_session_link(&state, &project, &mut workspace).await;

    // The missing linked session is not a planning session, so the helper
    // proceeds to prepare analysis state, which fails against the bogus path.
    assert!(
        result.is_err(),
        "expected an error from analysis preparation"
    );
    // The workspace link is left untouched on failure.
    assert!(workspace.linked_plan_branch_id.is_none());
}

// ── agent_workspace_pr_automation_defaults_for_project ────────────────────────

#[tokio::test]
async fn pr_automation_defaults_resolve_from_execution_settings() {
    let state = AppState::new_test();
    let project = Project::new("Demo".to_string(), "/tmp/demo".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");

    // Resolves the project's execution settings into workspace PR-automation
    // defaults without error (memory repo returns the default settings).
    let defaults = agent_workspace_pr_automation_defaults_for_project(&state, &project.id)
        .await
        .expect("defaults should resolve");

    let expected = AgentConversationWorkspacePrAutomationDefaults::from(
        &state
            .execution_settings_repo
            .get_settings(Some(&project.id))
            .await
            .unwrap(),
    );
    assert_eq!(defaults, expected);
}
