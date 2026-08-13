use crate::application::agent_issue_report::{
    build_agent_issue_report_draft, configured_support_issue_repository_from_yaml,
    resolve_agent_issue_report_destination_from_config_result, submit_agent_issue_report,
    submit_agent_issue_report_with_service, validate_github_repository,
    AgentIssueReportDestinationSource, AgentIssueReportEnvironment, BuildAgentIssueReportInput,
    SubmitAgentIssueReportInput,
};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::tests::mock_github_service::MockGithubService;
use crate::utils::runtime_log_paths;
use chrono::{TimeZone, Utc};
use std::io::{Error as IoError, ErrorKind};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

struct PathCleanup {
    path: PathBuf,
}

impl Drop for PathCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn fixed_environment() -> AgentIssueReportEnvironment {
    AgentIssueReportEnvironment {
        app_version: "0.42.0-test".to_string(),
        os_name: "macos".to_string(),
        os_version: Some("15.5".to_string()),
        arch: "aarch64".to_string(),
        generated_at: Utc
            .with_ymd_and_hms(2026, 6, 19, 12, 0, 0)
            .single()
            .expect("valid timestamp"),
    }
}

async fn seeded_report_context() -> (
    AppState,
    ChatConversationId,
    ProjectId,
    tempfile::TempDir,
    PathBuf,
    PathBuf,
) {
    let state = AppState::new_test();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_root = temp_dir.path().join("regulated-project");
    let workspace_root = temp_dir.path().join("agent-workspace");
    std::fs::create_dir_all(&project_root).expect("project root");
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let mut project = Project::new(
        "Regulated Project".to_string(),
        project_root.to_string_lossy().into_owned(),
    );
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should seed");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_title("Investigate failed agent run");
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "provider-session-123".to_string(),
    });
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should seed");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("abc123".to_string()),
        "ralphx/ralphx/agent-support".to_string(),
        workspace_root.to_string_lossy().into_owned(),
    );
    workspace.publication_pr_number = Some(448);
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should seed");

    (
        state,
        conversation_id,
        project_id,
        temp_dir,
        project_root,
        workspace_root,
    )
}

#[test]
fn configured_destination_prefers_support_issue_github_repository() {
    let yaml = r#"
support_issue:
  github_repository: aigentive/support
"#;

    assert_eq!(
        configured_support_issue_repository_from_yaml(yaml).as_deref(),
        Some("aigentive/support")
    );
}

#[test]
fn configured_destination_accepts_compat_issue_reporting_repository() {
    let yaml = r#"
issue_reporting:
  repository: enterprise/private-support
"#;

    assert_eq!(
        configured_support_issue_repository_from_yaml(yaml).as_deref(),
        Some("enterprise/private-support")
    );
}

#[test]
fn github_repository_validation_rejects_urls_and_nested_paths() {
    assert!(validate_github_repository("owner/repo").is_ok());
    assert!(validate_github_repository("https://github.com/owner/repo").is_err());
    assert!(validate_github_repository("owner/repo/extra").is_err());
}

#[test]
fn build_input_deserialization_uses_safe_log_defaults() {
    let input: BuildAgentIssueReportInput = serde_json::from_value(serde_json::json!({
        "conversationId": ChatConversationId::new().as_str(),
    }))
    .expect("build input should deserialize");

    assert!(input.include_logs);
    assert!(!input.recent_errors_only);
    assert_eq!(input.max_log_bytes, 24 * 1024);
}

#[test]
fn destination_resolution_uses_configured_repository_from_yaml() {
    let destination = resolve_agent_issue_report_destination_from_config_result(Ok(
        "support_issue:\n  github_repository: enterprise/support\n".to_string(),
    ));

    assert_eq!(destination.destination.repository, "enterprise/support");
    assert_eq!(
        destination.destination.source,
        AgentIssueReportDestinationSource::Configured
    );
    assert!(!destination.destination.is_default);
    assert!(destination.warnings.is_empty());
}

#[test]
fn destination_resolution_warns_for_invalid_configured_repository() {
    let destination = resolve_agent_issue_report_destination_from_config_result(Ok(
        "support_issue:\n  github_repository: https://github.com/owner/repo\n".to_string(),
    ));

    assert_eq!(destination.destination.repository, "aigentive/ralphx.app");
    assert_eq!(
        destination.destination.source,
        AgentIssueReportDestinationSource::PublicDefault
    );
    assert!(destination.destination.is_default);
    assert!(destination
        .warnings
        .iter()
        .any(|warning| warning.contains("Configured support issue repository is invalid")));
}

#[test]
fn destination_resolution_warns_when_config_is_missing() {
    let destination = resolve_agent_issue_report_destination_from_config_result(Err(IoError::new(
        ErrorKind::NotFound,
        "missing config",
    )));

    assert_eq!(destination.destination.repository, "aigentive/ralphx.app");
    assert_eq!(
        destination.destination.source,
        AgentIssueReportDestinationSource::PublicDefault
    );
    assert!(destination
        .warnings
        .iter()
        .any(|warning| warning.contains("using public default repository")));
}

#[tokio::test]
async fn submit_issue_report_uses_edited_markdown_body_exactly() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let github = MockGithubService::new();
    github.will_create_issue("https://github.com/aigentive/support/issues/42");

    let edited_body = "# Edited Report\n\nThe user removed one log line.";
    let body_dir = temp_dir.path().join("bodies");
    let issue_url = submit_agent_issue_report_with_service(
        &github,
        temp_dir.path(),
        &body_dir,
        "aigentive/support",
        "Support report",
        edited_body,
    )
    .await
    .expect("issue submit should succeed");

    assert_eq!(issue_url, "https://github.com/aigentive/support/issues/42");
    let state = github.state();
    assert_eq!(state.create_issue_calls, 1);
    assert_eq!(
        state
            .last_create_issue_args
            .as_ref()
            .map(|(repo, title, _)| (repo.as_str(), title.as_str())),
        Some(("aigentive/support", "Support report"))
    );
    assert_eq!(state.last_create_issue_body.as_deref(), Some(edited_body));
}

#[tokio::test]
async fn build_issue_report_includes_redacted_filtered_truncated_logs() {
    let (state, conversation_id, project_id, _temp_dir, project_root, workspace_root) =
        seeded_report_context().await;
    let stream_log = runtime_log_paths::stream_debug_log_file(&conversation_id.as_str());
    std::fs::create_dir_all(stream_log.parent().expect("stream log parent"))
        .expect("stream log directory");
    let _cleanup = PathCleanup {
        path: stream_log.clone(),
    };
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/Users/example"));
    let log_body = format!(
        "info should be filtered out\nwarn paths {} {} {} support@example.com\n{}\n",
        project_root.display(),
        workspace_root.display(),
        home.display(),
        "error repeated diagnostic line\n".repeat(220)
    );
    std::fs::write(&stream_log, log_body).expect("write stream log");

    let draft = build_agent_issue_report_draft(
        &state,
        BuildAgentIssueReportInput {
            conversation_id: conversation_id.as_str(),
            project_id: Some(project_id.as_str().to_string()),
            include_logs: true,
            recent_errors_only: true,
            max_log_bytes: 10,
        },
        fixed_environment(),
    )
    .await
    .expect("draft should build");

    assert_eq!(draft.conversation_id, conversation_id.as_str());
    assert_eq!(draft.project_id, project_id.as_str());
    assert_eq!(draft.generated_at, "2026-06-19T12:00:00+00:00");
    assert_eq!(draft.destination.repository, "aigentive/ralphx.app");
    assert!(matches!(
        draft.destination.source,
        AgentIssueReportDestinationSource::Configured
            | AgentIssueReportDestinationSource::PublicDefault
    ));
    assert!(draft.sources.iter().any(|source| {
        source.label.starts_with("stream-debug/")
            && source.included
            && source.truncated
            && source.detail.as_deref()
                == Some("Log content was truncated to the configured byte limit.")
    }));
    assert!(draft.markdown.contains("RalphX version: `0.42.0-test`"));
    assert!(draft.markdown.contains("Provider harness: `codex`"));
    assert!(draft
        .markdown
        .contains("Provider session ID: `provider-session-123`"));
    assert!(draft.markdown.contains("Agent mode: `edit`"));
    assert!(draft.markdown.contains("Publication PR number: `448`"));
    assert!(draft.markdown.contains("[PROJECT_ROOT]"));
    assert!(draft.markdown.contains("[AGENT_WORKSPACE]"));
    assert!(draft.markdown.contains("$HOME"));
    assert!(draft.markdown.contains("[REDACTED_EMAIL]"));
    assert!(!draft
        .markdown
        .contains(project_root.to_string_lossy().as_ref()));
    assert!(!draft
        .markdown
        .contains(workspace_root.to_string_lossy().as_ref()));
    assert!(!draft.markdown.contains("info should be filtered out"));
    assert!(draft
        .redaction_summary
        .replacements
        .iter()
        .any(|entry| { entry.category == "project_path" && entry.count >= 1 }));
    assert!(draft
        .redaction_summary
        .replacements
        .iter()
        .any(|entry| { entry.category == "workspace_path" && entry.count >= 1 }));
}

#[tokio::test]
async fn build_issue_report_records_log_omission_when_logs_disabled() {
    let (state, conversation_id, project_id, _temp_dir, _project_root, _workspace_root) =
        seeded_report_context().await;

    let draft = build_agent_issue_report_draft(
        &state,
        BuildAgentIssueReportInput {
            conversation_id: conversation_id.as_str(),
            project_id: Some(project_id.as_str().to_string()),
            include_logs: false,
            recent_errors_only: false,
            max_log_bytes: 24 * 1024,
        },
        fixed_environment(),
    )
    .await
    .expect("draft should build without logs");

    assert_eq!(
        draft.sources,
        vec![
            crate::application::agent_issue_report::AgentIssueReportSource {
                label: "logs".to_string(),
                included: false,
                truncated: false,
                detail: Some("Log inclusion disabled for this draft.".to_string()),
            }
        ]
    );
    assert!(draft.markdown.contains("_No logs included in this draft._"));
    assert!(draft
        .markdown
        .contains("- No automated redactions were applied."));
}

#[tokio::test]
async fn build_issue_report_includes_untruncated_full_log_without_detail() {
    let (state, conversation_id, project_id, _temp_dir, _project_root, _workspace_root) =
        seeded_report_context().await;
    let stream_log = runtime_log_paths::stream_debug_log_file(&conversation_id.as_str());
    std::fs::create_dir_all(stream_log.parent().expect("stream log parent"))
        .expect("stream log directory");
    let _cleanup = PathCleanup {
        path: stream_log.clone(),
    };
    let log_body = "info full log without trailing newline";
    std::fs::write(&stream_log, log_body).expect("write stream log");

    let draft = build_agent_issue_report_draft(
        &state,
        BuildAgentIssueReportInput {
            conversation_id: conversation_id.as_str(),
            project_id: Some(project_id.as_str().to_string()),
            include_logs: true,
            recent_errors_only: false,
            max_log_bytes: 24 * 1024,
        },
        fixed_environment(),
    )
    .await
    .expect("draft should build");

    let stream_source = draft
        .sources
        .iter()
        .find(|source| source.label.starts_with("stream-debug/"))
        .expect("stream log source should be included");
    assert!(stream_source.included);
    assert!(!stream_source.truncated);
    assert_eq!(stream_source.detail, None);
    assert!(draft.markdown.contains(log_body));
    assert!(draft
        .markdown
        .contains("info full log without trailing newline\n~~~"));
}

#[tokio::test]
async fn build_issue_report_collects_a_rotated_active_and_rolled_pair() {
    let (state, conversation_id, project_id, _temp_dir, _project_root, _workspace_root) =
        seeded_report_context().await;
    let log_root = runtime_log_paths::app_log_dir();
    std::fs::create_dir_all(&log_root).expect("app log directory");

    // Oldest first, so the rotated pair is the newest by modification time —
    // exactly how a launch that rotated appears on disk. Modification times are
    // stamped explicitly because write order alone ties on coarse filesystems.
    // Seeds are stamped ahead of now so they always rank newest relative to any
    // ambient dev logs in app_log_dir(), making the four-slot assertion deterministic.
    let seeded = [
        "ralphx_2026-08-11_08-00-00.log",
        "ralphx_2026-08-11_09-00-00.log",
        "ralphx_2026-08-11_10-00-00_rolled.log",
        "ralphx_2026-08-11_10-00-00.log",
    ];
    let oldest = SystemTime::now() + Duration::from_secs(3600);
    let mut _cleanups = Vec::new();
    for (index, name) in seeded.into_iter().enumerate() {
        let path = log_root.join(name);
        _cleanups.push(PathCleanup { path: path.clone() });
        std::fs::write(&path, format!("warn diagnostic line from {name}\n"))
            .expect("seeded launch log");
        let file = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open seeded launch log");
        file.set_times(
            std::fs::FileTimes::new()
                .set_modified(oldest + Duration::from_secs(60 * index as u64)),
        )
        .expect("stamp seeded launch log modification time");
    }

    let draft = build_agent_issue_report_draft(
        &state,
        BuildAgentIssueReportInput {
            conversation_id: conversation_id.as_str(),
            project_id: Some(project_id.as_str().to_string()),
            include_logs: true,
            recent_errors_only: false,
            max_log_bytes: 24 * 1024,
        },
        fixed_environment(),
    )
    .await
    .expect("draft should build");

    let labels: Vec<&str> = draft
        .sources
        .iter()
        .map(|source| source.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec![
            "ralphx_2026-08-11_10-00-00.log",
            "ralphx_2026-08-11_10-00-00_rolled.log",
            "ralphx_2026-08-11_09-00-00.log",
            "ralphx_2026-08-11_08-00-00.log",
        ],
        "the rotated pair must rank newest and keep its plain relative labels"
    );
    assert!(
        draft.sources.iter().all(|source| source.included),
        "every collected source must have a non-empty body"
    );
    assert!(draft
        .markdown
        .contains("warn diagnostic line from ralphx_2026-08-11_10-00-00_rolled.log"));
}

#[tokio::test]
async fn build_issue_report_rejects_project_context_mismatch() {
    let (state, conversation_id, _project_id, _temp_dir, _project_root, _workspace_root) =
        seeded_report_context().await;

    let err = build_agent_issue_report_draft(
        &state,
        BuildAgentIssueReportInput {
            conversation_id: conversation_id.as_str(),
            project_id: Some("different-project".to_string()),
            include_logs: false,
            recent_errors_only: false,
            max_log_bytes: 24 * 1024,
        },
        fixed_environment(),
    )
    .await
    .expect_err("mismatched project should fail");

    assert!(err
        .to_string()
        .contains("Selected project does not match the agent conversation workspace"));
}

#[tokio::test]
async fn build_issue_report_rejects_unknown_conversation() {
    let state = AppState::new_test();
    let err = build_agent_issue_report_draft(
        &state,
        BuildAgentIssueReportInput {
            conversation_id: ChatConversationId::new().as_str(),
            project_id: None,
            include_logs: true,
            recent_errors_only: false,
            max_log_bytes: 24 * 1024,
        },
        fixed_environment(),
    )
    .await
    .expect_err("unknown conversation should fail");

    assert!(err.to_string().contains("Agent conversation not found"));
}

#[tokio::test]
async fn submit_issue_report_validates_trims_and_truncates_user_input() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.will_create_issue("https://github.com/aigentive/ralphx.app/issues/99");
    state.github_service = Some(github.clone());
    let long_title = format!("  {}  ", "A".repeat(220));

    let response = submit_agent_issue_report(
        &state,
        SubmitAgentIssueReportInput {
            conversation_id: ChatConversationId::new().as_str(),
            repository: " aigentive/ralphx.app ".to_string(),
            title: long_title,
            body_markdown: "\n\n Edited report body \n".to_string(),
        },
    )
    .await
    .expect("issue submit should succeed");

    assert_eq!(response.repository, "aigentive/ralphx.app");
    assert_eq!(
        response.issue_url,
        "https://github.com/aigentive/ralphx.app/issues/99"
    );
    let state = github.state();
    let (repo, title, _) = state
        .last_create_issue_args
        .as_ref()
        .expect("issue args captured");
    assert_eq!(repo, "aigentive/ralphx.app");
    assert_eq!(title.len(), 180);
    assert!(title.chars().all(|ch| ch == 'A'));
    assert_eq!(
        state.last_create_issue_body.as_deref(),
        Some("Edited report body")
    );
}

#[tokio::test]
async fn submit_issue_report_rejects_invalid_user_inputs_before_github() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new().as_str();
    let cases = [
        (
            "not-a-valid-repo",
            "Title",
            "Body",
            "GitHub repository must be in owner/name format",
        ),
        (
            "owner/repo/extra",
            "Title",
            "Body",
            "GitHub repository must be a valid owner/name value",
        ),
        ("owner/repo", "   ", "Body", "Issue title cannot be empty"),
        ("owner/repo", "Title", "   ", "Issue body cannot be empty"),
    ];

    for (repository, title, body_markdown, expected) in cases {
        let err = submit_agent_issue_report(
            &state,
            SubmitAgentIssueReportInput {
                conversation_id: conversation_id.clone(),
                repository: repository.to_string(),
                title: title.to_string(),
                body_markdown: body_markdown.to_string(),
            },
        )
        .await
        .expect_err("invalid input should fail");
        assert!(
            err.to_string().contains(expected),
            "expected `{expected}` in `{err}`"
        );
    }
}

#[tokio::test]
async fn submit_issue_report_reports_missing_github_service_after_validation() {
    let state = AppState::new_test();

    let err = submit_agent_issue_report(
        &state,
        SubmitAgentIssueReportInput {
            conversation_id: ChatConversationId::new().as_str(),
            repository: "owner/repo".to_string(),
            title: "Support report".to_string(),
            body_markdown: "Body".to_string(),
        },
    )
    .await
    .expect_err("missing github service should fail");

    assert!(err
        .to_string()
        .contains("GitHub issue submission is unavailable"));
}
