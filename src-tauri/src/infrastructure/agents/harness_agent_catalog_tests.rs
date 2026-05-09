use super::{
    list_canonical_prompt_backed_agents, load_canonical_agent_definition,
    load_canonical_claude_metadata, load_canonical_codex_metadata, load_harness_agent_prompt,
    resolve_harness_agent_prompt_path, resolve_project_root_from_catalog_path,
    resolve_project_root_from_plugin_dir, try_load_canonical_claude_metadata, AgentPromptHarness,
};
use crate::infrastructure::agents::claude::get_agent_config;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

const PILOT_AGENTS: &[(&str, &str, &str)] = &[
    (
        "ralphx-ideation",
        "ideation_orchestrator",
        "ralphx-ideation",
    ),
    (
        "ralphx-ideation-team-lead",
        "ideation_team_lead",
        "ralphx-ideation-team-lead",
    ),
    (
        "ralphx-utility-session-namer",
        "session_namer",
        "ralphx-utility-session-namer",
    ),
    (
        "ralphx-utility-pr-describer",
        "pr_describer",
        "ralphx-utility-pr-describer",
    ),
];

const CODEX_PILOT_AGENTS: &[&str] = &[
    "ralphx-ideation",
    "ralphx-utility-session-namer",
    "ralphx-utility-pr-describer",
];
const CODEX_DELEGATION_GUIDE_AGENTS: &[&str] = &[
    "ralphx-chat-task",
    "ralphx-chat-project",
    "ralphx-ideation",
    "ralphx-ideation-readonly",
    "ralphx-plan-verifier",
    "ralphx-review-chat",
    "ralphx-review-history",
    "ralphx-execution-worker",
    "ralphx-execution-coder",
    "ralphx-execution-reviewer",
    "ralphx-execution-merger",
    "ralphx-qa-prep",
    "ralphx-qa-executor",
    "ralphx-research-deep-researcher",
];
const CLAUDE_ONLY_CANONICAL_AGENTS: &[(&str, &str, &str)] = &[(
    "ralphx-execution-team-lead",
    "worker_team_lead",
    "ralphx-execution-team-lead",
)];

const CROSS_HARNESS_VERIFICATION_AGENTS: &[(&str, &str, &str)] = &[
    (
        "ralphx-plan-verifier",
        "plan_verifier",
        "ralphx-plan-verifier",
    ),
    (
        "ralphx-plan-critic-completeness",
        "plan_critic_completeness",
        "ralphx-plan-critic-completeness",
    ),
    (
        "ralphx-plan-critic-implementation-feasibility",
        "plan_critic_implementation_feasibility",
        "ralphx-plan-critic-implementation-feasibility",
    ),
];

const CROSS_HARNESS_IDEATION_DELEGATE_AGENTS: &[(&str, &str, &str)] = &[
    (
        "ralphx-ideation-specialist-backend",
        "ideation_specialist_backend",
        "ralphx-ideation-specialist-backend",
    ),
    (
        "ralphx-ideation-specialist-frontend",
        "ideation_specialist_frontend",
        "ralphx-ideation-specialist-frontend",
    ),
    (
        "ralphx-ideation-specialist-infra",
        "ideation_specialist_infra",
        "ralphx-ideation-specialist-infra",
    ),
    (
        "ralphx-ideation-specialist-ux",
        "ideation_specialist_ux",
        "ralphx-ideation-specialist-ux",
    ),
    (
        "ralphx-ideation-specialist-code-quality",
        "ideation_specialist_code_quality",
        "ralphx-ideation-specialist-code-quality",
    ),
    (
        "ralphx-ideation-specialist-prompt-quality",
        "ideation_specialist_prompt_quality",
        "ralphx-ideation-specialist-prompt-quality",
    ),
    (
        "ralphx-ideation-specialist-intent",
        "ideation_specialist_intent",
        "ralphx-ideation-specialist-intent",
    ),
    (
        "ralphx-ideation-specialist-state-machine",
        "ideation_specialist_state_machine",
        "ralphx-ideation-specialist-state-machine",
    ),
    (
        "ralphx-ideation-specialist-pipeline-safety",
        "ideation_specialist_pipeline_safety",
        "ralphx-ideation-specialist-pipeline-safety",
    ),
    (
        "ralphx-ideation-advocate",
        "ideation_advocate",
        "ralphx-ideation-advocate",
    ),
    (
        "ralphx-ideation-critic",
        "ideation_critic",
        "ralphx-ideation-critic",
    ),
];

const CROSS_HARNESS_EXECUTION_AGENTS: &[(&str, &str, &str)] = &[
    ("ralphx-execution-reviewer", "reviewer", "reviewer"),
    ("ralphx-execution-merger", "merger", "merger"),
];

const CROSS_HARNESS_WORKFLOW_AGENTS: &[(&str, &str, &str)] = &[
    ("ralphx-execution-worker", "worker", "worker"),
    ("ralphx-execution-coder", "worker", "coder"),
    ("ralphx-review-chat", "review_chat", "review-chat"),
];

const CROSS_HARNESS_CHAT_AGENTS: &[(&str, &str, &str)] = &[
    ("ralphx-chat-task", "task_chat", "ralphx-chat-task"),
    ("ralphx-chat-project", "project_chat", "ralphx-chat-project"),
];

const CROSS_HARNESS_SUPPORT_AGENTS: &[(&str, &str, &str)] = &[
    ("ralphx-review-history", "review_history", "review-history"),
    (
        "ralphx-project-analyzer",
        "project_analyzer",
        "ralphx-project-analyzer",
    ),
    (
        "ralphx-memory-capture",
        "memory_capture",
        "ralphx-memory-capture",
    ),
    (
        "ralphx-memory-maintainer",
        "memory_maintainer",
        "ralphx-memory-maintainer",
    ),
];

const CROSS_HARNESS_GENERAL_AGENTS: &[(&str, &str, &str)] = &[
    (
        "ralphx-general-explorer",
        "general_explorer",
        "ralphx-general-explorer",
    ),
    (
        "ralphx-general-worker",
        "general_worker",
        "ralphx-general-worker",
    ),
    (
        "ralphx-agent-workspace-repair",
        "agent_workspace_repair",
        "ralphx-agent-workspace-repair",
    ),
    (
        "ralphx-research-deep-researcher",
        "researcher",
        "deep-researcher",
    ),
    (
        "ralphx-execution-orchestrator",
        "orchestrator",
        "orchestrator",
    ),
    ("ralphx-qa-prep", "qa_prep", "qa-prep"),
    ("ralphx-qa-executor", "qa_executor", "qa-executor"),
];

const CROSS_HARNESS_READONLY_IDEATION_AGENTS: &[(&str, &str, &str)] = &[(
    "ralphx-ideation-readonly",
    "ideation_orchestrator_readonly",
    "ralphx-ideation-readonly",
)];

const CANONICAL_MCP_TOOL_OWNED_AGENTS: &[&str] = &[
    "ralphx-general-explorer",
    "ralphx-general-worker",
    "ralphx-agent-workspace-repair",
    "ralphx-ideation",
    "ralphx-ideation-readonly",
    "ralphx-execution-worker",
    "ralphx-execution-coder",
    "ralphx-execution-reviewer",
    "ralphx-qa-executor",
    "ralphx-research-deep-researcher",
    "ralphx-execution-merger",
    "ralphx-memory-maintainer",
    "ralphx-memory-capture",
    "ralphx-ideation-team-lead",
    "ralphx-utility-session-namer",
    "ralphx-utility-pr-describer",
    "ralphx-chat-task",
    "ralphx-chat-project",
    "ralphx-review-chat",
    "ralphx-review-history",
    "ralphx-execution-orchestrator",
    "ralphx-project-analyzer",
    "ralphx-plan-verifier",
    "ralphx-plan-critic-completeness",
    "ralphx-plan-critic-implementation-feasibility",
    "ralphx-qa-prep",
    "ralphx-ideation-specialist-backend",
    "ralphx-ideation-specialist-frontend",
    "ralphx-ideation-specialist-infra",
    "ralphx-ideation-specialist-ux",
    "ralphx-ideation-specialist-code-quality",
    "ralphx-ideation-specialist-prompt-quality",
    "ralphx-ideation-specialist-intent",
    "ralphx-ideation-specialist-state-machine",
    "ralphx-ideation-specialist-pipeline-safety",
    "ralphx-ideation-advocate",
    "ralphx-ideation-critic",
];

const CANONICAL_CODEX_RUNTIME_FEATURE_OWNED_AGENTS: &[&str] = &[
    "ralphx-general-explorer",
    "ralphx-utility-pr-describer",
    "ralphx-plan-verifier",
    "ralphx-plan-critic-completeness",
    "ralphx-plan-critic-implementation-feasibility",
    "ralphx-qa-prep",
    "ralphx-ideation-specialist-backend",
    "ralphx-ideation-specialist-frontend",
    "ralphx-ideation-specialist-intent",
    "ralphx-ideation-specialist-pipeline-safety",
    "ralphx-ideation-specialist-prompt-quality",
    "ralphx-ideation-specialist-state-machine",
    "ralphx-ideation-specialist-ux",
    "ralphx-ideation-specialist-code-quality",
    "ralphx-ideation-advocate",
    "ralphx-ideation-critic",
];

const CANONICAL_CLAUDE_DISALLOWED_TOOL_OWNED_AGENTS: &[(&str, &[&str])] = &[
    (
        "ralphx-general-explorer",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation",
        &["Write", "Edit", "NotebookEdit", "Task(ralphx:*)"],
    ),
    (
        "ralphx-ideation-readonly",
        &["Write", "Edit", "NotebookEdit", "Task(ralphx:*)"],
    ),
    ("ralphx-plan-verifier", &["Write", "Edit", "NotebookEdit"]),
    (
        "ralphx-plan-critic-completeness",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-plan-critic-implementation-feasibility",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    ("ralphx-qa-prep", &["Write", "Edit", "Bash", "NotebookEdit"]),
    (
        "ralphx-ideation-specialist-backend",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-frontend",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-intent",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-pipeline-safety",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-prompt-quality",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-state-machine",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-ux",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-code-quality",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-specialist-infra",
        &["Write", "Edit", "NotebookEdit"],
    ),
    (
        "ralphx-ideation-advocate",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-ideation-critic",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
];

const CANONICAL_CLAUDE_HARNESS_OWNED_AGENTS: &[&str] = &[
    "ralphx-general-explorer",
    "ralphx-general-worker",
    "ralphx-agent-workspace-repair",
    "ralphx-execution-worker",
    "ralphx-execution-coder",
    "ralphx-execution-merger",
    "ralphx-memory-maintainer",
    "ralphx-memory-capture",
    "ralphx-utility-session-namer",
    "ralphx-chat-task",
    "ralphx-chat-project",
    "ralphx-review-chat",
    "ralphx-review-history",
    "ralphx-execution-orchestrator",
    "ralphx-project-analyzer",
    "ralphx-execution-reviewer",
    "ralphx-execution-team-lead",
    "ralphx-ideation",
    "ralphx-ideation-readonly",
    "ralphx-ideation-team-lead",
    "ralphx-plan-verifier",
    "ralphx-plan-critic-completeness",
    "ralphx-plan-critic-implementation-feasibility",
    "ralphx-qa-executor",
    "ralphx-qa-prep",
    "ralphx-research-deep-researcher",
    "ralphx-ideation-specialist-backend",
    "ralphx-ideation-specialist-code-quality",
    "ralphx-ideation-specialist-frontend",
    "ralphx-ideation-specialist-infra",
    "ralphx-ideation-specialist-intent",
    "ralphx-ideation-specialist-pipeline-safety",
    "ralphx-ideation-specialist-prompt-quality",
    "ralphx-ideation-specialist-state-machine",
    "ralphx-ideation-specialist-ux",
    "ralphx-ideation-advocate",
    "ralphx-ideation-critic",
];

const CANONICAL_CLAUDE_PERMISSION_MODE_OWNED_AGENTS: &[(&str, &str)] = &[
    ("ralphx-general-worker", "acceptEdits"),
    ("ralphx-agent-workspace-repair", "acceptEdits"),
    ("ralphx-execution-worker", "acceptEdits"),
    ("ralphx-execution-coder", "acceptEdits"),
    ("ralphx-execution-merger", "acceptEdits"),
    ("ralphx-execution-team-lead", "acceptEdits"),
    ("ralphx-qa-executor", "acceptEdits"),
    ("ralphx-memory-maintainer", "acceptEdits"),
    ("ralphx-memory-capture", "acceptEdits"),
];

const CANONICAL_CLAUDE_MODEL_OWNED_AGENTS: &[(&str, &str)] = &[
    ("ralphx-general-explorer", "sonnet"),
    ("ralphx-general-worker", "sonnet"),
    ("ralphx-agent-workspace-repair", "opus"),
    ("ralphx-utility-session-namer", "sonnet"),
    ("ralphx-chat-task", "sonnet"),
    ("ralphx-chat-project", "sonnet"),
    ("ralphx-review-chat", "sonnet"),
    ("ralphx-review-history", "sonnet"),
    ("ralphx-execution-orchestrator", "opus"),
    ("ralphx-project-analyzer", "sonnet"),
    ("ralphx-execution-worker", "sonnet"),
    ("ralphx-execution-coder", "sonnet"),
    ("ralphx-execution-reviewer", "sonnet"),
    ("ralphx-execution-merger", "opus"),
    ("ralphx-execution-team-lead", "sonnet"),
    ("ralphx-ideation", "opus"),
    ("ralphx-ideation-readonly", "opus"),
    ("ralphx-ideation-team-lead", "opus"),
    ("ralphx-plan-verifier", "opus"),
    ("ralphx-plan-critic-completeness", "opus"),
    ("ralphx-plan-critic-implementation-feasibility", "opus"),
    ("ralphx-qa-prep", "sonnet"),
    ("ralphx-qa-executor", "sonnet"),
    ("ralphx-research-deep-researcher", "opus"),
    ("ralphx-ideation-specialist-backend", "opus"),
    ("ralphx-ideation-specialist-frontend", "opus"),
    ("ralphx-ideation-specialist-infra", "opus"),
    ("ralphx-ideation-specialist-ux", "opus"),
    ("ralphx-ideation-specialist-code-quality", "opus"),
    ("ralphx-ideation-specialist-prompt-quality", "opus"),
    ("ralphx-ideation-specialist-intent", "opus"),
    ("ralphx-ideation-specialist-state-machine", "opus"),
    ("ralphx-ideation-specialist-pipeline-safety", "opus"),
    ("ralphx-ideation-advocate", "opus"),
    ("ralphx-ideation-critic", "opus"),
    ("ralphx-memory-maintainer", "sonnet"),
    ("ralphx-memory-capture", "sonnet"),
];

const CANONICAL_CLAUDE_EFFORT_OWNED_AGENTS: &[(&str, &str)] = &[("ralphx-ideation", "max")];

const CANONICAL_CLAUDE_TOOL_SPEC_OWNED_AGENTS: &[(&str, &str, &[&str], bool)] = &[
    ("ralphx-general-explorer", "readonly_tools", &[], false),
    (
        "ralphx-general-worker",
        "readonly_tools",
        &["Write", "Edit", "Bash", "LSP"],
        false,
    ),
    (
        "ralphx-agent-workspace-repair",
        "base_tools",
        &["Edit"],
        false,
    ),
    ("ralphx-chat-task", "base_tools", &["Task"], false),
    ("ralphx-chat-project", "readonly_tools", &[], false),
    ("ralphx-review-chat", "base_tools", &["Task"], false),
    ("ralphx-review-history", "base_tools", &["Task"], false),
    (
        "ralphx-execution-orchestrator",
        "base_tools",
        &["Write", "Edit", "Task"],
        false,
    ),
    ("ralphx-project-analyzer", "base_tools", &[], false),
    ("ralphx-ideation", "base_tools", &["Task"], false),
    ("ralphx-ideation-readonly", "base_tools", &["Task"], false),
    (
        "ralphx-ideation-team-lead",
        "base_tools",
        &[
            "Task",
            "TaskStop",
            "TeamCreate",
            "TeamDelete",
            "SendMessage",
        ],
        false,
    ),
    (
        "ralphx-execution-worker",
        "base_tools",
        &["Write", "Edit", "LSP"],
        false,
    ),
    (
        "ralphx-execution-coder",
        "base_tools",
        &["Write", "Edit", "Task", "LSP"],
        false,
    ),
    ("ralphx-execution-reviewer", "base_tools", &[], false),
    ("ralphx-qa-prep", "base_tools", &["Task"], false),
    (
        "ralphx-qa-executor",
        "base_tools",
        &["Write", "Edit", "Task"],
        false,
    ),
    ("ralphx-execution-merger", "base_tools", &["Edit"], false),
    (
        "ralphx-execution-team-lead",
        "base_tools",
        &[
            "Write",
            "Edit",
            "Task",
            "LSP",
            "TaskStop",
            "TeamCreate",
            "TeamDelete",
            "SendMessage",
        ],
        false,
    ),
    (
        "ralphx-plan-critic-completeness",
        "critic_tools",
        &[],
        false,
    ),
    (
        "ralphx-plan-critic-implementation-feasibility",
        "critic_tools",
        &[],
        false,
    ),
    (
        "ralphx-research-deep-researcher",
        "base_tools",
        &["Write", "Task"],
        false,
    ),
    (
        "ralphx-memory-maintainer",
        "base_tools",
        &["Write", "Edit"],
        false,
    ),
    (
        "ralphx-memory-capture",
        "base_tools",
        &["Write", "Edit"],
        false,
    ),
    (
        "ralphx-ideation-specialist-backend",
        "base_tools",
        &[],
        false,
    ),
    (
        "ralphx-ideation-specialist-frontend",
        "base_tools",
        &[],
        false,
    ),
    ("ralphx-ideation-specialist-infra", "base_tools", &[], false),
    ("ralphx-ideation-specialist-ux", "base_tools", &[], false),
    (
        "ralphx-ideation-specialist-code-quality",
        "base_tools",
        &[],
        false,
    ),
    (
        "ralphx-ideation-specialist-prompt-quality",
        "base_tools",
        &[],
        false,
    ),
    (
        "ralphx-ideation-specialist-intent",
        "base_tools",
        &[],
        false,
    ),
    (
        "ralphx-ideation-specialist-state-machine",
        "base_tools",
        &[],
        false,
    ),
    (
        "ralphx-ideation-specialist-pipeline-safety",
        "base_tools",
        &[],
        false,
    ),
    ("ralphx-ideation-advocate", "base_tools", &[], false),
    ("ralphx-ideation-critic", "base_tools", &[], false),
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("canonical repo root")
}

#[test]
fn lexical_project_root_with_parent_segments_loads_canonical_prompts() {
    let lexical_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");

    let prompt =
        load_harness_agent_prompt(&lexical_root, "ralphx-ideation", AgentPromptHarness::Claude);

    assert!(
        prompt.is_some(),
        "lexical repo roots like src-tauri/.. should still resolve canonical prompts"
    );
}

#[test]
fn codex_runtime_features_load_from_harness_metadata() {
    let root = project_root();

    let verifier = load_canonical_codex_metadata(&root, "ralphx-plan-verifier");
    assert_eq!(
        verifier.runtime_features.get("shell_tool"),
        Some(&false),
        "verifier should disable Codex shell_tool declaratively"
    );

    let backend_specialist =
        load_canonical_codex_metadata(&root, "ralphx-ideation-specialist-backend");
    assert_eq!(
        backend_specialist.runtime_features.get("shell_tool"),
        Some(&false),
        "Claude no-Bash specialist should map to Codex shell_tool=false"
    );

    let pr_describer = load_canonical_codex_metadata(&root, "ralphx-utility-pr-describer");
    assert_eq!(
        pr_describer.runtime_features.get("shell_tool"),
        Some(&false),
        "PR describer should disable Codex shell_tool declaratively"
    );

    let session_namer = load_canonical_codex_metadata(&root, "ralphx-utility-session-namer");
    assert_eq!(
        session_namer.runtime_features.get("shell_tool"),
        Some(&false),
        "session namer should disable Codex shell_tool declaratively"
    );
}

#[test]
fn project_chat_codex_surface_can_advance_ideation_send_message_actions() {
    let root = project_root();
    let metadata = load_canonical_codex_metadata(&root, "ralphx-chat-project");
    let prompt = load_harness_agent_prompt(&root, "ralphx-chat-project", AgentPromptHarness::Codex)
        .expect("missing codex prompt for ralphx-chat-project");

    assert!(
        metadata
            .mcp_tools
            .iter()
            .any(|tool| tool == "v1_send_ideation_message"),
        "project chat must be able to advance external MCP next_action=send_message flows"
    );
    assert!(
        prompt.contains("next_action` yourself") && prompt.contains("v1_send_ideation_message"),
        "project chat prompt must tell the agent to consume send_message actions itself"
    );
}

#[test]
fn project_chat_codex_surface_can_append_to_open_ideation_plans() {
    let root = project_root();
    let metadata = load_canonical_codex_metadata(&root, "ralphx-chat-project");
    let prompt = load_harness_agent_prompt(&root, "ralphx-chat-project", AgentPromptHarness::Codex)
        .expect("missing codex prompt for ralphx-chat-project");

    assert_eq!(metadata.mcp_transport.as_deref(), Some("external"));
    assert!(
        metadata
            .mcp_tools
            .iter()
            .any(|tool| tool == "v1_append_task_to_plan"),
        "project chat Codex must use the external append tool for accepted ideation follow-ups"
    );
    assert!(
        prompt.contains("v1_append_task_to_plan") && prompt.contains("waiting-on-PR"),
        "project chat Codex prompt must describe append behavior for open accepted ideation plans"
    );
}

#[test]
fn project_chat_claude_surface_uses_external_ideation_tools() {
    let root = project_root();
    let metadata = load_canonical_claude_metadata(&root, "ralphx-chat-project");
    let prompt =
        load_harness_agent_prompt(&root, "ralphx-chat-project", AgentPromptHarness::Claude)
            .expect("missing claude prompt for ralphx-chat-project");

    assert_eq!(metadata.mcp_transport.as_deref(), Some("external"));
    assert!(
        metadata
            .mcp_tools
            .iter()
            .any(|tool| tool == "v1_start_ideation"),
        "project chat Claude must be able to start external MCP ideation runs"
    );
    assert!(
        prompt.contains("v1_start_ideation") && prompt.contains("next_action` yourself"),
        "project chat Claude prompt must describe the external ideation flow"
    );
}

#[test]
fn project_chat_claude_surface_can_append_to_open_ideation_plans() {
    let root = project_root();
    let metadata = load_canonical_claude_metadata(&root, "ralphx-chat-project");
    let prompt =
        load_harness_agent_prompt(&root, "ralphx-chat-project", AgentPromptHarness::Claude)
            .expect("missing claude prompt for ralphx-chat-project");

    assert_eq!(metadata.mcp_transport.as_deref(), Some("external"));
    assert!(
        metadata
            .mcp_tools
            .iter()
            .any(|tool| tool == "v1_append_task_to_plan"),
        "project chat Claude must use the external append tool for accepted ideation follow-ups"
    );
    assert!(
        prompt.contains("v1_append_task_to_plan") && prompt.contains("waiting-on-PR"),
        "project chat Claude prompt must describe append behavior for open accepted ideation plans"
    );
}

#[test]
fn project_chat_codex_surface_can_advance_ideation_send_message_actions() {
    let root = project_root();
    let metadata = load_canonical_codex_metadata(&root, "ralphx-chat-project");
    let prompt = load_harness_agent_prompt(&root, "ralphx-chat-project", AgentPromptHarness::Codex)
        .expect("missing codex prompt for ralphx-chat-project");

    assert!(
        metadata
            .mcp_tools
            .iter()
            .any(|tool| tool == "v1_send_ideation_message"),
        "project chat must be able to advance external MCP next_action=send_message flows"
    );
    assert!(
        prompt.contains("next_action` yourself") && prompt.contains("v1_send_ideation_message"),
        "project chat prompt must tell the agent to consume send_message actions itself"
    );
}

#[test]
fn project_chat_claude_surface_uses_external_ideation_tools() {
    let root = project_root();
    let metadata = load_canonical_claude_metadata(&root, "ralphx-chat-project");
    let prompt =
        load_harness_agent_prompt(&root, "ralphx-chat-project", AgentPromptHarness::Claude)
            .expect("missing claude prompt for ralphx-chat-project");

    assert_eq!(metadata.mcp_transport.as_deref(), Some("external"));
    assert!(
        metadata
            .mcp_tools
            .iter()
            .any(|tool| tool == "v1_start_ideation"),
        "project chat Claude must be able to start external MCP ideation runs"
    );
    assert!(
        prompt.contains("v1_start_ideation") && prompt.contains("next_action` yourself"),
        "project chat Claude prompt must describe the external ideation flow"
    );
}

#[test]
fn codex_runtime_features_prefer_root_agent_metadata_over_legacy_harness_file() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let agent_dir = temp.path().join("agents/test-agent");
    fs::create_dir_all(agent_dir.join("codex")).expect("agent dirs should exist");
    fs::write(
        agent_dir.join("agent.yaml"),
        r#"name: test-agent
role: test_role
harnesses:
  codex:
    runtime_features:
      shell_tool: false
"#,
    )
    .expect("root agent metadata should write");
    fs::write(
        agent_dir.join("codex/agent.yaml"),
        r#"runtime_features:
  shell_tool: true
"#,
    )
    .expect("legacy codex metadata should write");

    let metadata = load_canonical_codex_metadata(temp.path(), "test-agent");
    assert_eq!(
        metadata.runtime_features.get("shell_tool"),
        Some(&false),
        "root canonical codex runtime features should override legacy codex/agent.yaml metadata"
    );
}

#[test]
fn canonical_codex_runtime_features_match_loader_for_current_owned_agents() {
    let root = project_root();

    for agent_name in CANONICAL_CODEX_RUNTIME_FEATURE_OWNED_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        let runtime_features = load_canonical_codex_metadata(&root, agent_name);

        assert_eq!(
            definition.harnesses.codex.runtime_features, runtime_features.runtime_features,
            "root harnesses.codex.runtime_features should stay aligned for {agent_name}"
        );
    }
}

#[test]
fn canonical_claude_metadata_merges_root_and_legacy_harness_fields() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let agent_dir = temp.path().join("agents/test-agent");
    fs::create_dir_all(agent_dir.join("claude")).expect("agent dirs should exist");
    fs::write(
        agent_dir.join("agent.yaml"),
        r#"name: test-agent
role: test_role
harnesses:
  claude:
    disallowed_tools:
      - Write
"#,
    )
    .expect("root agent metadata should write");
    fs::write(
        agent_dir.join("claude/agent.yaml"),
        r#"disallowed_tools:
  - Bash
max_turns: 12
"#,
    )
    .expect("legacy claude metadata should write");

    let metadata = try_load_canonical_claude_metadata(temp.path(), "test-agent")
        .expect("claude metadata should load");
    assert_eq!(metadata.disallowed_tools, vec!["Write"]);
    assert_eq!(metadata.max_turns, Some(12));
}

#[test]
fn canonical_claude_disallowed_tools_match_loader_for_current_owned_agents() {
    let root = project_root();

    for (agent_name, expected_disallowed_tools) in CANONICAL_CLAUDE_DISALLOWED_TOOL_OWNED_AGENTS {
        let metadata = try_load_canonical_claude_metadata(&root, agent_name)
            .unwrap_or_else(|_| panic!("expected Claude metadata for {agent_name}"));

        assert_eq!(
            metadata.disallowed_tools,
            expected_disallowed_tools
                .iter()
                .map(|tool| (*tool).to_string())
                .collect::<Vec<_>>(),
            "root canonical Claude disallowed_tools should stay aligned for {agent_name}"
        );
    }
}

#[test]
fn canonical_claude_harness_metadata_matches_loader_for_owned_agents() {
    let root = project_root();

    for agent_name in CANONICAL_CLAUDE_HARNESS_OWNED_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        let metadata = try_load_canonical_claude_metadata(&root, agent_name)
            .unwrap_or_else(|_| panic!("expected Claude metadata for {agent_name}"));

        assert_eq!(
            definition.harnesses.claude, metadata,
            "root canonical Claude harness metadata should stay aligned for {agent_name}"
        );
    }
}

#[test]
fn canonical_claude_permission_mode_matches_loader_for_owned_agents() {
    let root = project_root();

    for (agent_name, expected_permission_mode) in CANONICAL_CLAUDE_PERMISSION_MODE_OWNED_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        let metadata = try_load_canonical_claude_metadata(&root, agent_name).unwrap_or_else(|_| {
            panic!("failed to load canonical Claude metadata for {agent_name}")
        });

        assert_eq!(
            definition.harnesses.claude.permission_mode.as_deref(),
            Some(*expected_permission_mode),
            "root harnesses.claude.permission_mode should stay aligned for {agent_name}"
        );
        assert_eq!(
            metadata.permission_mode.as_deref(),
            Some(*expected_permission_mode),
            "Claude metadata loader should preserve canonical permission_mode for {agent_name}"
        );
        assert_eq!(
            get_agent_config(agent_name)
                .unwrap_or_else(|| panic!("missing runtime config for {agent_name}"))
                .permission_mode
                .as_deref(),
            Some(*expected_permission_mode),
            "runtime config should prefer canonical permission_mode for {agent_name}"
        );
    }
}

#[test]
fn canonical_claude_model_matches_loader_for_owned_agents() {
    let root = project_root();

    for (agent_name, expected_model) in CANONICAL_CLAUDE_MODEL_OWNED_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        let metadata = try_load_canonical_claude_metadata(&root, agent_name).unwrap_or_else(|_| {
            panic!("failed to load canonical Claude metadata for {agent_name}")
        });

        assert_eq!(
            definition.harnesses.claude.model.as_deref(),
            Some(*expected_model),
            "root harnesses.claude.model should stay aligned for {agent_name}"
        );
        assert_eq!(
            metadata.model.as_deref(),
            Some(*expected_model),
            "Claude metadata loader should preserve canonical model for {agent_name}"
        );
        assert_eq!(
            get_agent_config(agent_name)
                .unwrap_or_else(|| panic!("missing runtime config for {agent_name}"))
                .model
                .as_deref(),
            Some(*expected_model),
            "runtime config should prefer canonical model for {agent_name}"
        );
    }
}

#[test]
fn canonical_claude_effort_matches_loader_for_owned_agents() {
    let root = project_root();

    for (agent_name, expected_effort) in CANONICAL_CLAUDE_EFFORT_OWNED_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        let metadata = try_load_canonical_claude_metadata(&root, agent_name).unwrap_or_else(|_| {
            panic!("failed to load canonical Claude metadata for {agent_name}")
        });

        assert_eq!(
            definition.harnesses.claude.effort.as_deref(),
            Some(*expected_effort),
            "root harnesses.claude.effort should stay aligned for {agent_name}"
        );
        assert_eq!(
            metadata.effort.as_deref(),
            Some(*expected_effort),
            "Claude metadata loader should preserve canonical effort for {agent_name}"
        );
        assert_eq!(
            get_agent_config(agent_name)
                .unwrap_or_else(|| panic!("missing runtime config for {agent_name}"))
                .effort
                .as_deref(),
            Some(*expected_effort),
            "runtime config should prefer canonical effort for {agent_name}"
        );
    }
}

#[test]
fn canonical_claude_tool_spec_matches_loader_for_owned_agents() {
    let root = project_root();

    for (agent_name, expected_extends, expected_include, expected_mcp_only) in
        CANONICAL_CLAUDE_TOOL_SPEC_OWNED_AGENTS
    {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        let metadata = try_load_canonical_claude_metadata(&root, agent_name).unwrap_or_else(|_| {
            panic!("failed to load canonical Claude metadata for {agent_name}")
        });
        let spec = definition
            .harnesses
            .claude
            .tools
            .as_ref()
            .unwrap_or_else(|| panic!("expected canonical Claude tools for {agent_name}"));

        assert_eq!(spec.extends.as_deref(), Some(*expected_extends));
        assert_eq!(
            spec.include,
            expected_include
                .iter()
                .map(|v| (*v).to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(spec.mcp_only, *expected_mcp_only);
        assert_eq!(metadata.tools.as_ref(), Some(spec));
        assert_eq!(
            get_agent_config(agent_name)
                .unwrap_or_else(|| panic!("missing runtime config for {agent_name}"))
                .mcp_only,
            *expected_mcp_only
        );
    }
}

#[test]
fn canonical_mcp_tools_match_runtime_yaml_for_current_owned_agents() {
    let root = project_root();

    for agent_name in CANONICAL_MCP_TOOL_OWNED_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        let runtime_config = get_agent_config(agent_name)
            .unwrap_or_else(|| panic!("expected runtime config for {agent_name}"));

        assert_eq!(
            definition.capabilities.mcp_tools,
            runtime_config.allowed_mcp_tools,
            "canonical capabilities.mcp_tools should stay aligned with ralphx.yaml runtime grants for {agent_name}"
        );
    }
}

#[test]
fn pr_describer_codex_surface_uses_shared_prompt_and_submit_tool() {
    let root = project_root();
    let definition = load_canonical_agent_definition(&root, "ralphx-utility-pr-describer")
        .expect("expected canonical PR describer definition");
    let prompt = load_harness_agent_prompt(
        &root,
        "ralphx-utility-pr-describer",
        AgentPromptHarness::Codex,
    )
    .expect("expected PR describer Codex prompt");
    let metadata = load_canonical_codex_metadata(&root, "ralphx-utility-pr-describer");

    assert_eq!(
        definition.capabilities.mcp_tools,
        vec!["submit_agent_workspace_pr_description".to_string()]
    );
    assert!(
        prompt.contains("submit_agent_workspace_pr_description"),
        "Codex PR describer prompt should expose the submit contract"
    );
    assert!(
        !prompt.contains("mcp__ralphx__"),
        "Codex PR describer prompt should not use Claude-style MCP names"
    );
    assert!(
        !prompt.contains("truncated"),
        "Codex PR describer prompt should not tell helpers to expose bounded-context truncation"
    );
    assert_eq!(metadata.runtime_features.get("shell_tool"), Some(&false));
}

#[test]
fn project_analyzer_codex_prompt_uses_harness_neutral_file_inspection_language() {
    let root = project_root();
    let prompt =
        load_harness_agent_prompt(&root, "ralphx-project-analyzer", AgentPromptHarness::Codex)
            .expect("expected project analyzer Codex prompt");

    assert!(
        !prompt.contains("Use `Glob`"),
        "project analyzer Codex prompt should not prescribe Claude-only Glob tool usage"
    );
    assert!(
        !prompt.contains("use `Read`"),
        "project analyzer Codex prompt should not prescribe Claude-only Read tool usage"
    );
    assert!(
        prompt.contains("Inspect the working directory"),
        "project analyzer should express file inspection in harness-neutral terms"
    );
}

fn canonical_agent_names(root: &std::path::Path) -> Vec<String> {
    let mut names = std::fs::read_dir(root.join("agents"))
        .expect("canonical agents dir should exist")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_type = entry.file_type().ok()?;
            if file_type.is_dir() {
                Some(entry.file_name().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn live_runtime_agents() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut agents = Vec::new();
    agents.extend_from_slice(PILOT_AGENTS);
    agents.extend_from_slice(CLAUDE_ONLY_CANONICAL_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_VERIFICATION_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_IDEATION_DELEGATE_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_EXECUTION_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_WORKFLOW_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_CHAT_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_SUPPORT_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_GENERAL_AGENTS);
    agents.extend_from_slice(CROSS_HARNESS_READONLY_IDEATION_AGENTS);
    agents
}

#[test]
fn pilot_agent_definitions_load_from_canonical_tree() {
    let root = project_root();

    for (agent_name, role, _) in PILOT_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CLAUDE_ONLY_CANONICAL_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_VERIFICATION_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_IDEATION_DELEGATE_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_EXECUTION_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_WORKFLOW_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_CHAT_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_SUPPORT_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_GENERAL_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }

    for (agent_name, role, _) in CROSS_HARNESS_READONLY_IDEATION_AGENTS {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, *agent_name);
        assert_eq!(definition.role, *role);
    }
}

#[test]
fn pilot_agent_prompt_paths_exist_for_both_harnesses() {
    let root = project_root();

    for agent_name in CODEX_PILOT_AGENTS {
        let claude_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude);
        assert!(
            claude_path.is_some(),
            "expected claude prompt path for {agent_name}"
        );
        let codex_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex);
        assert!(
            codex_path.is_some(),
            "expected codex prompt path for {agent_name}"
        );

        if matches!(
            *agent_name,
            "ralphx-utility-session-namer" | "ralphx-utility-pr-describer"
        ) {
            assert!(
                claude_path.as_ref().is_some_and(
                    |path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))
                ),
                "expected {agent_name} claude prompt to resolve through shared/prompt.md"
            );
            assert!(
                codex_path.as_ref().is_some_and(
                    |path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))
                ),
                "expected {agent_name} codex prompt to resolve through shared/prompt.md"
            );
        }
    }

    assert!(
        resolve_harness_agent_prompt_path(
            &root,
            "ralphx-ideation-team-lead",
            AgentPromptHarness::Claude
        )
        .is_some(),
        "expected claude prompt path for ralphx-ideation-team-lead"
    );
    assert!(
        resolve_harness_agent_prompt_path(&root, "ralphx-ideation-team-lead", AgentPromptHarness::Codex)
            .is_none(),
        "ralphx-ideation-team-lead should not have a codex prompt while Codex team mode is unsupported"
    );

    for (agent_name, _, _) in CLAUDE_ONLY_CANONICAL_AGENTS {
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude)
                .is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex)
                .is_none(),
            "{agent_name} should remain claude-only until a codex prompt exists"
        );
    }

    for (agent_name, _, _) in CROSS_HARNESS_VERIFICATION_AGENTS {
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude)
                .is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex)
                .is_some(),
            "expected codex prompt path for {agent_name}"
        );
    }

    for (agent_name, _, _) in CROSS_HARNESS_IDEATION_DELEGATE_AGENTS {
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude)
                .is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex)
                .is_some(),
            "expected codex prompt path for {agent_name}"
        );
    }

    for (agent_name, _, _) in CROSS_HARNESS_EXECUTION_AGENTS {
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude)
                .is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex)
                .is_some(),
            "expected codex prompt path for {agent_name}"
        );
    }

    for (agent_name, _, _) in CROSS_HARNESS_WORKFLOW_AGENTS {
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude)
                .is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex)
                .is_some(),
            "expected codex prompt path for {agent_name}"
        );
    }

    for (agent_name, _, _) in CROSS_HARNESS_CHAT_AGENTS {
        let claude_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude);
        let codex_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex);
        assert!(
            claude_path.is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            codex_path.is_some(),
            "expected codex prompt path for {agent_name}"
        );
        if *agent_name == "ralphx-chat-project" {
            assert!(
                claude_path.as_ref().is_some_and(
                    |path| path.ends_with("agents/ralphx-chat-project/claude/prompt.md")
                ),
                "expected ralphx-chat-project claude prompt to resolve through claude/prompt.md"
            );
            assert!(
                codex_path.as_ref().is_some_and(
                    |path| path.ends_with("agents/ralphx-chat-project/codex/prompt.md")
                ),
                "expected ralphx-chat-project codex prompt to resolve through codex/prompt.md"
            );
        } else {
            assert!(
                claude_path.as_ref().is_some_and(
                    |path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))
                ),
                "expected {agent_name} claude prompt to resolve through shared/prompt.md"
            );
            assert!(
                codex_path.as_ref().is_some_and(
                    |path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))
                ),
                "expected {agent_name} codex prompt to resolve through shared/prompt.md"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_SUPPORT_AGENTS {
        let claude_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude);
        let codex_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex);
        assert!(
            claude_path.is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            codex_path.is_some(),
            "expected codex prompt path for {agent_name}"
        );
        assert!(
            claude_path
                .as_ref()
                .is_some_and(|path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))),
            "expected {agent_name} claude prompt to resolve through shared/prompt.md"
        );
        assert!(
            codex_path
                .as_ref()
                .is_some_and(|path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))),
            "expected {agent_name} codex prompt to resolve through shared/prompt.md"
        );
    }

    for (agent_name, _, _) in CROSS_HARNESS_GENERAL_AGENTS {
        let claude_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude);
        let codex_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex);
        assert!(
            claude_path.is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            codex_path.is_some(),
            "expected codex prompt path for {agent_name}"
        );
        assert!(
            claude_path
                .as_ref()
                .is_some_and(|path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))),
            "expected {agent_name} claude prompt to resolve through shared/prompt.md"
        );
        assert!(
            codex_path
                .as_ref()
                .is_some_and(|path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))),
            "expected {agent_name} codex prompt to resolve through shared/prompt.md"
        );
    }

    for (agent_name, _, _) in CROSS_HARNESS_READONLY_IDEATION_AGENTS {
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude)
                .is_some(),
            "expected claude prompt path for {agent_name}"
        );
        assert!(
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex)
                .is_some(),
            "expected codex prompt path for {agent_name}"
        );
    }
}

#[test]
fn legacy_agent_aliases_resolve_into_canonical_catalog_entries() {
    let root = project_root();
    let cases = [
        ("orchestrator-ideation", "ralphx-ideation"),
        ("plan-verifier", "ralphx-plan-verifier"),
        ("ralphx-worker", "ralphx-execution-worker"),
        ("session-namer", "ralphx-utility-session-namer"),
        ("pr-describer", "ralphx-utility-pr-describer"),
    ];

    for (legacy_name, canonical_name) in cases {
        let definition = load_canonical_agent_definition(&root, legacy_name).unwrap_or_else(|| {
            panic!("expected canonical definition for legacy alias {legacy_name}")
        });
        assert_eq!(definition.name, canonical_name);
        assert!(
            resolve_harness_agent_prompt_path(&root, legacy_name, AgentPromptHarness::Claude)
                .is_some(),
            "expected Claude prompt for legacy alias {legacy_name}"
        );
    }
}

#[test]
fn live_runtime_agents_resolve_through_canonical_claude_prompts() {
    let root = project_root();

    for (agent_name, _, _) in live_runtime_agents() {
        assert!(
            load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Claude).is_some(),
            "missing canonical Claude prompt for {agent_name}"
        );
    }
}

#[test]
fn canonical_claude_runtime_agent_list_matches_live_runtime_roster() {
    let root = project_root();
    let expected = live_runtime_agents()
        .into_iter()
        .map(|(agent_name, _, _)| agent_name.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let actual = list_canonical_prompt_backed_agents(&root, AgentPromptHarness::Claude)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert!(
        !actual.contains("ralphx-ideation-team-member"),
        "compatibility teammate target should not be treated as a prompt-backed Claude runtime agent"
    );
}

#[test]
fn codex_pilot_prompts_are_frontmatter_free() {
    let root = project_root();

    for agent_name in CODEX_PILOT_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        assert!(
            !prompt.starts_with("---"),
            "codex prompt for {agent_name} must not carry Claude frontmatter"
        );
        assert!(
            !prompt.contains("\nmcpServers:"),
            "codex prompt for {agent_name} must not carry Claude mcpServers frontmatter"
        );
    }
}

#[test]
fn codex_pilot_prompts_avoid_claude_only_team_and_task_syntax() {
    let root = project_root();
    let banned_terms = [
        "Task(",
        "TeamCreate",
        "TaskCreate",
        "TaskUpdate",
        "TaskGet",
        "TaskList",
        "TaskOutput",
        "TaskStop",
        "SendMessage",
        "mcpServers",
        "mcp__ralphx__",
        "CLAUDE_PLUGIN_ROOT",
        "--append-system-prompt",
    ];

    for agent_name in CODEX_PILOT_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }
}

#[test]
fn codex_ideation_pilot_prompts_declare_codex_native_delegation_contract() {
    let root = project_root();

    let orchestrator =
        load_harness_agent_prompt(&root, "ralphx-ideation", AgentPromptHarness::Codex)
            .expect("missing codex orchestrator prompt");
    assert!(
        orchestrator.contains("Codex-native delegation"),
        "orchestrator codex prompt should describe Codex-native delegation semantics"
    );
    assert!(
        load_harness_agent_prompt(
            &root,
            "ralphx-ideation-team-lead",
            AgentPromptHarness::Codex
        )
        .is_none(),
        "ralphx-ideation-team-lead should not have a codex prompt while team mode is unsupported"
    );
}

#[test]
fn codex_plan_verifier_prompt_treats_actionable_needs_revision_as_non_terminal() {
    let root = project_root();
    let prompt =
        load_harness_agent_prompt(&root, "ralphx-plan-verifier", AgentPromptHarness::Codex)
            .expect("missing codex prompt for ralphx-plan-verifier");

    assert!(
        prompt.contains(
            "actionable `needs_revision` is non-terminal until you have a terminal `convergence_reason`"
        ),
        "codex verifier prompt must keep actionable needs_revision non-terminal"
    );
    assert!(
        prompt.contains(
            "if terminal cleanup rejects actionable `needs_revision` because `convergence_reason` is missing"
        ),
        "codex verifier prompt must continue the loop when terminal cleanup rejects actionable needs_revision"
    );
}

#[test]
fn codex_ideation_prompt_prioritizes_explicit_reverify_requests() {
    let root = project_root();
    let prompt = load_harness_agent_prompt(&root, "ralphx-ideation", AgentPromptHarness::Codex)
        .expect("missing codex prompt for ralphx-ideation");

    assert!(
        prompt.contains("If the user explicitly asks to re-run or start a fresh verification round"),
        "codex ideation prompt must prioritize explicit rerun requests over stale terminal verification results"
    );
    assert!(
        prompt.contains(
            "start the fresh verification child instead of summarizing blockers and reopening choices"
        ),
        "codex ideation prompt must not reopen planning choices when the user already requested a rerun"
    );
}

#[test]
fn codex_ideation_prompt_keeps_provider_resume_silent_by_default() {
    let root = project_root();
    let prompt = load_harness_agent_prompt(&root, "ralphx-ideation", AgentPromptHarness::Codex)
        .expect("missing codex prompt for ralphx-ideation");

    assert!(
        prompt.contains("Do not behave like recovery mode on normal follow-up turns"),
        "codex ideation prompt must keep provider_resume turns conversational by default"
    );
    assert!(
        prompt.contains("Do not narrate routine refreshes to the user unless the check changes the answer"),
        "codex ideation prompt must avoid user-facing recovery chatter on ordinary resumed follow-ups"
    );
}

#[test]
fn codex_spawn_capable_prompts_reference_explicit_delegation_tools() {
    let root = project_root();

    for agent_name in CODEX_DELEGATION_GUIDE_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        assert!(
            prompt.contains("delegate_start"),
            "codex prompt for {agent_name} should mention delegate_start"
        );
        assert!(
            prompt.contains("delegate_wait"),
            "codex prompt for {agent_name} should mention delegate_wait"
        );
        assert!(
            prompt.contains("delegate_cancel"),
            "codex prompt for {agent_name} should mention delegate_cancel"
        );
    }
}

#[test]
fn canonical_delegation_policy_appendix_is_injected_only_for_delegating_agents() {
    let root = project_root();

    for agent_name in CODEX_DELEGATION_GUIDE_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        assert!(
            prompt.contains("## RalphX Delegation Policy (AUTO-GENERATED)"),
            "delegating codex prompt for {agent_name} should include the generated delegation appendix"
        );
    }

    let session_namer = load_harness_agent_prompt(
        &root,
        "ralphx-utility-session-namer",
        AgentPromptHarness::Codex,
    )
    .expect("missing session namer codex prompt");
    assert!(
        !session_namer.contains("## RalphX Delegation Policy (AUTO-GENERATED)"),
        "non-delegating codex prompt should not include the generated delegation appendix"
    );
}

#[test]
fn generated_delegation_appendix_describes_general_explorer_usage_for_general_explorer_callers() {
    let root = project_root();

    for agent_name in [
        "ralphx-chat-task",
        "ralphx-chat-project",
        "ralphx-ideation",
        "ralphx-ideation-readonly",
        "ralphx-review-chat",
        "ralphx-review-history",
        "ralphx-execution-coder",
        "ralphx-execution-reviewer",
        "ralphx-execution-merger",
        "ralphx-qa-prep",
        "ralphx-qa-executor",
        "ralphx-research-deep-researcher",
    ] {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        assert!(
            prompt.contains("`ralphx-general-explorer`: read-only exploration delegate"),
            "delegation appendix for {agent_name} should describe the general explorer target"
        );
        assert!(
            prompt.contains(
                "bounded file inspection, pattern search, or evidence gathering without edits"
            ),
            "delegation appendix for {agent_name} should explain when to use the general explorer"
        );
    }
}

#[test]
fn resolve_project_root_from_generated_plugin_dir_finds_repo_agents_tree() {
    let root = project_root();
    let generated_plugin_dir = root.join(".artifacts/generated/claude-plugin");

    assert_eq!(
        resolve_project_root_from_plugin_dir(&generated_plugin_dir),
        root,
        "generated plugin dir should resolve back to the repo root so canonical agent definitions load in live Codex/Claude runs"
    );
}

#[test]
fn resolve_project_root_from_config_dir_finds_repo_agents_tree() {
    let root = project_root();
    let config_dir = root.join("config");

    assert_eq!(
        resolve_project_root_from_catalog_path(&config_dir),
        Some(root),
        "config-rooted canonical discovery should resolve back to the repo root so agent-config loading does not depend on plugin-dir heuristics"
    );
}

#[test]
fn resolve_project_root_from_traversal_like_plugin_dir_skips_directory_scans() {
    let invalid_plugin_dir = PathBuf::from("../plugins/app");

    assert_eq!(
        resolve_project_root_from_plugin_dir(&invalid_plugin_dir),
        PathBuf::from("../plugins"),
        "plugin dirs with parent traversal components should be rejected before filesystem scans"
    );
}

#[test]
fn resolve_project_root_from_symlinked_base_plugin_dir_finds_repo_agents_tree() {
    let root = project_root();
    let temp = tempdir().expect("tempdir");
    let plugin_dir = temp.path().join("plugins/app");
    fs::create_dir_all(plugin_dir.parent().expect("plugin dir parent"))
        .expect("create temp plugins parent");
    symlink_dir(root.join("plugins/app"), &plugin_dir);

    assert_eq!(
        resolve_project_root_from_plugin_dir(&plugin_dir),
        root,
        "symlinked plugin dirs should resolve back to the RalphX repo root so canonical agent definitions load outside the source checkout"
    );
}

#[test]
fn resolve_project_root_from_external_generated_plugin_dir_follows_runtime_symlinks() {
    let root = project_root();
    let temp = tempdir().expect("tempdir");
    let generated_plugin_dir = temp.path().join("generated/claude-plugin");
    fs::create_dir_all(&generated_plugin_dir).expect("create generated plugin dir");
    symlink_dir(
        root.join("plugins/app/ralphx-mcp-server"),
        generated_plugin_dir.join("ralphx-mcp-server"),
    );

    assert_eq!(
        resolve_project_root_from_plugin_dir(&generated_plugin_dir),
        root,
        "external generated plugin dirs should resolve through symlinked runtime entries back to the RalphX repo root"
    );
}

#[test]
fn invalid_agent_names_do_not_escape_canonical_agent_tree() {
    let root = project_root();

    assert!(
        load_canonical_agent_definition(&root, "../escape").is_none(),
        "path-like agent names must not resolve to canonical agent definitions"
    );
    assert!(
        resolve_harness_agent_prompt_path(&root, "../escape", AgentPromptHarness::Claude).is_none(),
        "path-like agent names must not resolve to harness prompt files"
    );
}

#[test]
fn agents_root_symlink_escaping_project_root_is_rejected() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("project");
    let outside_agents = temp.path().join("outside-agents");
    fs::create_dir_all(&root).expect("create project root");
    seed_minimal_canonical_claude_agent(&outside_agents.join("ralphx-escape"), "ralphx-escape");
    symlink_dir(&outside_agents, root.join("agents"));

    assert!(
        load_canonical_agent_definition(&root, "ralphx-escape").is_none(),
        "agents roots that resolve outside the project must be rejected"
    );
    assert!(
        load_harness_agent_prompt(&root, "ralphx-escape", AgentPromptHarness::Claude).is_none(),
        "escaped agents roots must not resolve harness prompts"
    );
    assert!(
        list_canonical_prompt_backed_agents(&root, AgentPromptHarness::Claude).is_empty(),
        "escaped agents roots must not populate the canonical prompt-backed agent list"
    );
}

#[test]
fn canonical_agent_directory_symlink_escaping_project_root_is_rejected() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("project");
    let outside_agent = temp.path().join("outside-agent");
    fs::create_dir_all(root.join("agents")).expect("create project agents dir");
    seed_minimal_canonical_claude_agent(&outside_agent, "ralphx-escape");
    symlink_dir(&outside_agent, root.join("agents/ralphx-escape"));

    assert!(
        load_canonical_agent_definition(&root, "ralphx-escape").is_none(),
        "canonical agent directories that resolve outside the project must be rejected"
    );
    assert!(
        resolve_harness_agent_prompt_path(&root, "ralphx-escape", AgentPromptHarness::Claude)
            .is_none(),
        "escaped canonical agent directories must not resolve harness prompt paths"
    );
    assert!(
        list_canonical_prompt_backed_agents(&root, AgentPromptHarness::Claude).is_empty(),
        "escaped canonical agent directories must not populate the canonical prompt-backed agent list"
    );
}

#[test]
fn codex_execution_prompts_avoid_claude_only_team_and_task_syntax() {
    let root = project_root();
    let banned_terms = [
        "Task(",
        "TaskCreate",
        "TaskUpdate",
        "TaskGet",
        "TaskList",
        "TaskOutput",
        "TaskStop",
        "TeamCreate",
        "TeamDelete",
        "SendMessage",
        "mcpServers",
        "mcp__ralphx__",
        "CLAUDE_PLUGIN_ROOT",
        "--append-system-prompt",
    ];

    for (agent_name, _, _) in CROSS_HARNESS_EXECUTION_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_WORKFLOW_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_CHAT_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_SUPPORT_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_GENERAL_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_READONLY_IDEATION_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_VERIFICATION_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }

    for (agent_name, _, _) in CROSS_HARNESS_IDEATION_DELEGATE_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        for banned in banned_terms {
            assert!(
                !prompt.contains(banned),
                "codex prompt for {agent_name} must not contain Claude-only syntax `{banned}`"
            );
        }
    }
}

#[cfg(unix)]
fn symlink_dir(source: impl AsRef<std::path::Path>, target: impl AsRef<std::path::Path>) {
    std::os::unix::fs::symlink(source, target).expect("create directory symlink");
}

#[cfg(windows)]
fn symlink_dir(source: impl AsRef<std::path::Path>, target: impl AsRef<std::path::Path>) {
    std::os::windows::fs::symlink_dir(source, target).expect("create directory symlink");
}

fn seed_minimal_canonical_claude_agent(agent_root: &std::path::Path, agent_name: &str) {
    fs::create_dir_all(agent_root.join("claude")).expect("create canonical claude dir");
    fs::write(
        agent_root.join("agent.yaml"),
        format!("name: {agent_name}\nrole: test_agent\n"),
    )
    .expect("write shared definition");
    fs::write(
        agent_root.join("claude/prompt.md"),
        format!("prompt for {agent_name}"),
    )
    .expect("write claude prompt");
}

#[test]
fn canonical_agent_tree_is_schema_valid_and_loadable() {
    let root = project_root();
    let prompt_backed_agents =
        list_canonical_prompt_backed_agents(&root, AgentPromptHarness::Claude)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
    let promptless_metadata_only_agents = std::collections::BTreeSet::from([
        "ralphx-execution-team-member".to_string(),
        "ralphx-ideation-team-member".to_string(),
    ]);

    for agent_name in canonical_agent_names(&root) {
        let definition = load_canonical_agent_definition(&root, &agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert_eq!(definition.name, agent_name);

        try_load_canonical_claude_metadata(&root, &agent_name)
            .unwrap_or_else(|error| panic!("invalid claude metadata for {agent_name}: {error}"));

        let shared_prompt_path = root
            .join("agents")
            .join(&agent_name)
            .join("shared/prompt.md");
        let claude_prompt_path = root
            .join("agents")
            .join(&agent_name)
            .join("claude/prompt.md");
        let codex_prompt_path = root
            .join("agents")
            .join(&agent_name)
            .join("codex/prompt.md");

        let has_any_prompt = shared_prompt_path.exists()
            || claude_prompt_path.exists()
            || codex_prompt_path.exists();
        if has_any_prompt {
            continue;
        }

        assert!(
            promptless_metadata_only_agents.contains(&agent_name),
            "canonical agent {agent_name} must define at least one prompt unless it is an explicit metadata-only compatibility entry"
        );
        assert!(
            !prompt_backed_agents.contains(&agent_name),
            "metadata-only compatibility agent {agent_name} must not be treated as a prompt-backed Claude runtime agent"
        );
    }
}
