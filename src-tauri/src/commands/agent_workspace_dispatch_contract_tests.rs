//! Contract tests binding backend-generated agent assignments to the live MCP surface.
//!
//! Production incident 2026-07-31: durable redelivery addressed the generic workspace repairer
//! with a message telling it to call `complete_agent_workspace_pr_fix` — a tool that agent does
//! not hold — while the repairer's own completion tool rejected the fields the backend demanded.
//! Both failures are invisible to ordinary unit tests because each side is internally consistent.
//! These tests fail when a backend message names a tool its recipient cannot call.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::application::agent_workspace_publish_recovery::{
    due_pr_autofix_redispatch_message, due_repair_dispatch_message,
};
use crate::application::services::pr_merge_poller::{
    build_agent_workspace_pr_autofix_message, classify_agent_workspace_pr_autofix_issue,
};
use crate::commands::unified_chat_commands::{
    build_agent_workspace_repair_message_for_target, AgentConversationWorkspaceRepairTarget,
    AgentWorkspacePostRepairAction,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairSource, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::services::github_service::{PrHealth, PrHealthCheck, PrMergeableState};
use crate::domain::services::{PrStatus, PrSyncState};
use crate::infrastructure::agents::claude::agent_names::{
    SHORT_AGENT_WORKSPACE_PR_FIXER, SHORT_AGENT_WORKSPACE_REPAIR,
};
use crate::infrastructure::agents::harness_agent_catalog::load_canonical_agent_definition;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("canonical repo root")
}

/// The exact tool names the named canonical agent is granted, as RalphX will register them.
fn canonical_mcp_tools(agent_name: &str) -> BTreeSet<String> {
    let definition = load_canonical_agent_definition(&project_root(), agent_name)
        .unwrap_or_else(|| panic!("canonical agent definition for {agent_name}"));
    definition
        .capabilities
        .mcp_tools
        .into_iter()
        .collect::<BTreeSet<_>>()
}

/// Every RalphX completion/context tool a message instructs its recipient to call.
fn referenced_agent_workspace_tools(message: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for prefix in ["complete_agent_workspace_", "get_agent_workspace_"] {
        let mut rest = message;
        while let Some(start) = rest.find(prefix) {
            let tail = &rest[start..];
            let end = tail
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(tail.len());
            found.insert(tail[..end].to_string());
            rest = &tail[end..];
        }
    }
    found
}

fn contract_workspace() -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("contract-conversation".to_string()),
        ProjectId::from_string("contract-project".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/contract".to_string(),
        "/tmp/ralphx-contract-workspace".to_string(),
    );
    workspace.publication_pr_number = Some(4242);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/4242".to_string());
    workspace
}

fn contract_attempt(source: AgentWorkspaceRepairSource) -> AgentWorkspaceRepairAttempt {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("contract-conversation".to_string()),
        source,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.pr_autofix_health_fingerprint =
        Some("github_pr_autofix:4242:checks:rust-tests".to_string());
    attempt.pending_reasons = vec!["CI is failing on the published PR".to_string()];
    attempt
}

fn failing_pr_health() -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: None,
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "ralphx/test/contract".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some("contract-head".to_string()),
            base_ref_oid: Some("base-sha".to_string()),
        },
        review_decision: None,
        checks: vec![PrHealthCheck {
            name: "Rust tests".to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("failure".to_string()),
            details_url: Some("https://github.com/owner/repo/actions/runs/1".to_string()),
        }],
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }
}

/// Each backend assignment, paired with the canonical agent it is actually addressed to.
fn backend_assignments() -> Vec<(&'static str, &'static str, String)> {
    let workspace = contract_workspace();
    let health = failing_pr_health();
    let issue = classify_agent_workspace_pr_autofix_issue(4242, &health)
        .expect("a failing check classifies as a PR autofix issue");

    vec![
        (
            "durable repair redelivery",
            SHORT_AGENT_WORKSPACE_REPAIR,
            due_repair_dispatch_message(
                &contract_attempt(AgentWorkspaceRepairSource::Publish),
                &workspace,
            ),
        ),
        (
            "durable PR autofix redelivery",
            SHORT_AGENT_WORKSPACE_PR_FIXER,
            due_pr_autofix_redispatch_message(
                &contract_attempt(AgentWorkspaceRepairSource::PrAutofix),
                &workspace,
            ),
        ),
        (
            "poller PR autofix first dispatch",
            SHORT_AGENT_WORKSPACE_PR_FIXER,
            build_agent_workspace_pr_autofix_message(
                4242,
                workspace.publication_pr_url.as_deref(),
                "agent workspace",
                &workspace,
                &issue,
            ),
        ),
        (
            "publish repair request",
            SHORT_AGENT_WORKSPACE_REPAIR,
            build_agent_workspace_repair_message_for_target(
                "push rejected: base moved",
                &workspace,
                &AgentConversationWorkspaceRepairTarget {
                    branch_name: workspace.branch_name.clone(),
                    base_ref: workspace.base_ref.clone(),
                    base_display_name: workspace.base_display_name.clone(),
                    worktree_path: None,
                },
                AgentWorkspacePostRepairAction::Publish,
            ),
        ),
    ]
}

#[test]
fn every_backend_assignment_names_only_tools_its_recipient_agent_holds() {
    for (label, agent_name, message) in backend_assignments() {
        let granted = canonical_mcp_tools(agent_name);
        let referenced = referenced_agent_workspace_tools(&message);
        assert!(
            !referenced.is_empty(),
            "{label}: an assignment that names no completion tool cannot be completed\n{message}"
        );
        for tool in &referenced {
            assert!(
                granted.contains(tool),
                "{label}: assignment addressed to {agent_name} calls `{tool}`, which is not in its \
                 canonical capabilities.mcp_tools ({granted:?})\n{message}"
            );
        }
    }
}

/// The incident's exact shape: a PR-autofix redelivery that reaches the generic repairer.
#[test]
fn pr_autofix_and_generic_repair_assignments_do_not_share_a_completion_tool() {
    let workspace = contract_workspace();
    let pr_fix = referenced_agent_workspace_tools(&due_pr_autofix_redispatch_message(
        &contract_attempt(AgentWorkspaceRepairSource::PrAutofix),
        &workspace,
    ));
    let generic = referenced_agent_workspace_tools(&due_repair_dispatch_message(
        &contract_attempt(AgentWorkspaceRepairSource::Publish),
        &workspace,
    ));

    assert!(
        pr_fix.contains("complete_agent_workspace_pr_fix"),
        "PR autofix redelivery must route completion through the PR fixer's tool: {pr_fix:?}"
    );
    assert!(
        !canonical_mcp_tools(SHORT_AGENT_WORKSPACE_REPAIR)
            .contains("complete_agent_workspace_pr_fix"),
        "the generic repairer must not hold the PR fixer's completion tool"
    );
    assert!(
        !generic.contains("complete_agent_workspace_pr_fix"),
        "the generic repair assignment must never name the PR fixer's completion tool: {generic:?}"
    );
}

/// The repair prompt and the backend both demand `resolution`; the tool schema that accepts it
/// lives in the MCP server, so the canonical grant must at least still name the tool the prompt
/// tells the agent to call.
#[test]
fn repair_prompt_completion_tool_is_granted_to_the_repair_agent() {
    let prompt = std::fs::read_to_string(
        project_root().join("agents/ralphx-agent-workspace-repair/shared/prompt.md"),
    )
    .expect("workspace repair prompt");
    let referenced = referenced_agent_workspace_tools(&prompt);
    let granted = canonical_mcp_tools(SHORT_AGENT_WORKSPACE_REPAIR);
    assert!(
        referenced.contains("complete_agent_workspace_repair"),
        "the repair prompt must name its completion tool: {referenced:?}"
    );
    for tool in &referenced {
        assert!(
            granted.contains(tool),
            "the repair prompt calls `{tool}`, which is not granted to {SHORT_AGENT_WORKSPACE_REPAIR}"
        );
    }
}
