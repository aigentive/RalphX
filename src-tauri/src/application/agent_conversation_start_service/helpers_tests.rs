//! Unit tests for the pure helpers extracted from the agent conversation start
//! service. These cover input shaping, mode/base-ref parsing, source pull
//! request normalization, workspace-decision predicates, and effort selection.

use super::*;
use crate::application::agent_conversation_start_service::AgentWorkspaceSourcePullRequestInput;
use crate::domain::agents::LogicalEffort;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest, IdeationAnalysisBaseRefKind,
};

fn pr_input(number: i64, head_ref_name: &str) -> AgentWorkspaceSourcePullRequestInput {
    AgentWorkspaceSourcePullRequestInput {
        number,
        url: Some("  https://example.com/pr/1  ".to_string()),
        title: Some("  Some PR  ".to_string()),
        head_ref_name: head_ref_name.to_string(),
        base_ref_name: Some("  main  ".to_string()),
        head_ref_oid: Some("  abc123  ".to_string()),
    }
}

// ----- parse_agent_workspace_mode -----

#[test]
fn parse_mode_defaults_to_edit_when_none() {
    assert_eq!(
        parse_agent_workspace_mode(None).unwrap(),
        AgentConversationWorkspaceMode::Edit
    );
}

#[test]
fn parse_mode_defaults_to_edit_when_blank_or_whitespace() {
    assert_eq!(
        parse_agent_workspace_mode(Some("   ")).unwrap(),
        AgentConversationWorkspaceMode::Edit
    );
    assert_eq!(
        parse_agent_workspace_mode(Some("")).unwrap(),
        AgentConversationWorkspaceMode::Edit
    );
}

#[test]
fn parse_mode_trims_and_parses_known_values() {
    assert_eq!(
        parse_agent_workspace_mode(Some("  chat  ")).unwrap(),
        AgentConversationWorkspaceMode::Chat
    );
    assert_eq!(
        parse_agent_workspace_mode(Some("plan")).unwrap(),
        AgentConversationWorkspaceMode::Plan
    );
    assert_eq!(
        parse_agent_workspace_mode(Some("ideation")).unwrap(),
        AgentConversationWorkspaceMode::Ideation
    );
    assert_eq!(
        parse_agent_workspace_mode(Some("review_pr")).unwrap(),
        AgentConversationWorkspaceMode::ReviewPr
    );
}

#[test]
fn parse_mode_rejects_unknown_value() {
    let err = parse_agent_workspace_mode(Some("nonsense")).unwrap_err();
    assert!(
        err.contains("nonsense"),
        "error should echo the bad value: {err}"
    );
}

// ----- parse_agent_workspace_base_kind -----

#[test]
fn parse_base_kind_none_for_missing_or_blank() {
    assert_eq!(parse_agent_workspace_base_kind(None).unwrap(), None);
    assert_eq!(parse_agent_workspace_base_kind(Some("")).unwrap(), None);
    assert_eq!(parse_agent_workspace_base_kind(Some("   ")).unwrap(), None);
}

#[test]
fn parse_base_kind_trims_and_parses_known_values() {
    assert_eq!(
        parse_agent_workspace_base_kind(Some("  project_default  ")).unwrap(),
        Some(IdeationAnalysisBaseRefKind::ProjectDefault)
    );
    assert_eq!(
        parse_agent_workspace_base_kind(Some("current_branch")).unwrap(),
        Some(IdeationAnalysisBaseRefKind::CurrentBranch)
    );
    assert_eq!(
        parse_agent_workspace_base_kind(Some("local_branch")).unwrap(),
        Some(IdeationAnalysisBaseRefKind::LocalBranch)
    );
    assert_eq!(
        parse_agent_workspace_base_kind(Some("pull_request")).unwrap(),
        Some(IdeationAnalysisBaseRefKind::PullRequest)
    );
}

#[test]
fn parse_base_kind_rejects_unknown_value() {
    let err = parse_agent_workspace_base_kind(Some("weird")).unwrap_err();
    assert!(
        err.contains("weird"),
        "error should echo the bad value: {err}"
    );
}

// ----- trim_optional_input -----

#[test]
fn trim_optional_input_returns_none_for_none() {
    assert_eq!(trim_optional_input(None), None);
}

#[test]
fn trim_optional_input_returns_none_for_blank() {
    assert_eq!(trim_optional_input(Some("   ".to_string())), None);
    assert_eq!(trim_optional_input(Some(String::new())), None);
}

#[test]
fn trim_optional_input_trims_surrounding_whitespace() {
    assert_eq!(
        trim_optional_input(Some("  value  ".to_string())),
        Some("value".to_string())
    );
}

// ----- normalize_agent_workspace_source_pull_request -----

#[test]
fn normalize_pr_none_input_returns_none() {
    let result = normalize_agent_workspace_source_pull_request(
        None,
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("feature"),
    )
    .unwrap();
    assert_eq!(result, None);
}

#[test]
fn normalize_pr_rejects_non_positive_number() {
    for bad in [0_i64, -1] {
        let err = normalize_agent_workspace_source_pull_request(
            Some(pr_input(bad, "feature")),
            Some(IdeationAnalysisBaseRefKind::LocalBranch),
            Some("feature"),
        )
        .unwrap_err();
        assert!(err.contains("must be positive"), "got: {err}");
    }
}

#[test]
fn normalize_pr_requires_local_branch_base_kind() {
    // Wrong kind.
    let err = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "feature")),
        Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        Some("feature"),
    )
    .unwrap_err();
    assert!(err.contains("local_branch"), "got: {err}");

    // Missing kind entirely.
    let err = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "feature")),
        None,
        Some("feature"),
    )
    .unwrap_err();
    assert!(err.contains("local_branch"), "got: {err}");
}

#[test]
fn normalize_pr_requires_non_empty_head_ref() {
    let err = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "   ")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        None,
    )
    .unwrap_err();
    assert!(err.contains("head branch is required"), "got: {err}");
}

#[test]
fn normalize_pr_requires_base_ref_to_match_head_when_present() {
    let err = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "feature")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("a-different-branch"),
    )
    .unwrap_err();
    assert!(
        err.contains("must match the selected base ref"),
        "got: {err}"
    );
}

#[test]
fn normalize_pr_allows_blank_base_ref_without_match_check() {
    // A blank/whitespace base ref is filtered out, so no match check applies.
    let result = normalize_agent_workspace_source_pull_request(
        Some(pr_input(7, "feature")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("   "),
    )
    .unwrap()
    .expect("should produce a normalized PR");
    assert_eq!(result.head_ref_name, "feature");
}

#[test]
fn normalize_pr_trims_optional_fields_and_keeps_head_ref() {
    let result = normalize_agent_workspace_source_pull_request(
        Some(pr_input(42, "  feature  ")),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("  feature  "),
    )
    .unwrap()
    .expect("should produce a normalized PR");

    let expected = AgentWorkspaceSourcePullRequest {
        number: 42,
        url: Some("https://example.com/pr/1".to_string()),
        title: Some("Some PR".to_string()),
        head_ref_name: "feature".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("abc123".to_string()),
    };
    assert_eq!(result, expected);
}

#[test]
fn normalize_pr_drops_blank_optional_fields() {
    let input = AgentWorkspaceSourcePullRequestInput {
        number: 9,
        url: Some("   ".to_string()),
        title: None,
        head_ref_name: "feature".to_string(),
        base_ref_name: Some(String::new()),
        head_ref_oid: None,
    };
    let result = normalize_agent_workspace_source_pull_request(
        Some(input),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        None,
    )
    .unwrap()
    .expect("should produce a normalized PR");
    assert_eq!(result.url, None);
    assert_eq!(result.title, None);
    assert_eq!(result.base_ref_name, None);
    assert_eq!(result.head_ref_oid, None);
}

// ----- agent_mode_requires_workspace -----

#[test]
fn requires_workspace_true_for_non_chat_modes() {
    for mode in [
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr,
    ] {
        assert!(
            agent_mode_requires_workspace(mode),
            "{mode:?} should require a workspace"
        );
    }
}

#[test]
fn requires_workspace_false_for_chat() {
    assert!(!agent_mode_requires_workspace(
        AgentConversationWorkspaceMode::Chat
    ));
}

// ----- agent_mode_should_create_workspace -----

#[test]
fn should_create_workspace_true_for_workspace_modes_without_pr() {
    assert!(agent_mode_should_create_workspace(
        AgentConversationWorkspaceMode::Edit,
        None
    ));
    assert!(agent_mode_should_create_workspace(
        AgentConversationWorkspaceMode::Plan,
        None
    ));
}

#[test]
fn should_create_workspace_false_for_chat_without_pr() {
    assert!(!agent_mode_should_create_workspace(
        AgentConversationWorkspaceMode::Chat,
        None
    ));
}

#[test]
fn should_create_workspace_true_for_chat_with_source_pr() {
    let pr = AgentWorkspaceSourcePullRequest {
        number: 1,
        url: None,
        title: None,
        head_ref_name: "feature".to_string(),
        base_ref_name: None,
        head_ref_oid: None,
    };
    assert!(agent_mode_should_create_workspace(
        AgentConversationWorkspaceMode::Chat,
        Some(&pr)
    ));
}

// ----- normalized_effort_for_supported -----

#[test]
fn normalized_effort_keeps_requested_when_supported() {
    let supported = [LogicalEffort::Low, LogicalEffort::High];
    assert_eq!(
        normalized_effort_for_supported(Some(LogicalEffort::High), &supported, LogicalEffort::Low),
        LogicalEffort::High
    );
}

#[test]
fn normalized_effort_falls_back_to_default_when_requested_unsupported() {
    let supported = [LogicalEffort::Low, LogicalEffort::Medium];
    assert_eq!(
        normalized_effort_for_supported(
            Some(LogicalEffort::Max),
            &supported,
            LogicalEffort::Medium
        ),
        LogicalEffort::Medium
    );
}

#[test]
fn normalized_effort_uses_default_when_requested_none() {
    let supported = [LogicalEffort::Low, LogicalEffort::Medium];
    assert_eq!(
        normalized_effort_for_supported(None, &supported, LogicalEffort::Low),
        LogicalEffort::Low
    );
}
