use super::{
    list_canonical_agent_names, list_canonical_prompt_backed_agents,
    load_canonical_agent_definition, load_canonical_agent_definition_for_profile,
    load_canonical_claude_metadata, load_canonical_claude_metadata_for_profile,
    load_canonical_codex_metadata, load_canonical_codex_metadata_for_profile,
    load_harness_agent_prompt, load_harness_agent_prompt_for_profile,
    render_agent_runtime_profile_context, resolve_harness_agent_prompt_path,
    resolve_project_root_from_catalog_path, resolve_project_root_from_plugin_dir,
    trusted_canonical_profile_name, try_load_canonical_claude_metadata,
    try_load_canonical_claude_metadata_for_profile, AgentPromptHarness,
};
use crate::infrastructure::agents::claude::{
    get_agent_config, get_agent_config_for_profile, get_preapproved_tools_for_profile,
};
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
        "ralphx-utility-session-namer",
        "session_namer",
        "ralphx-utility-session-namer",
    ),
    (
        "ralphx-utility-pr-describer",
        "pr_describer",
        "ralphx-utility-pr-describer",
    ),
    (
        "ralphx-utility-plan-complexity",
        "plan_complexity_assessor",
        "ralphx-utility-plan-complexity",
    ),
    (
        "ralphx-workspace-reviewer",
        "workspace_reviewer",
        "ralphx-workspace-reviewer",
    ),
    (
        "ralphx-automation-setup",
        "automation-setup",
        "ralphx-automation-setup",
    ),
];

const CODEX_PILOT_AGENTS: &[&str] = &[
    "ralphx-ideation",
    "ralphx-utility-session-namer",
    "ralphx-utility-pr-describer",
    "ralphx-utility-plan-complexity",
    "ralphx-workspace-reviewer",
    "ralphx-automation-setup",
];
const CODEX_DELEGATION_GUIDE_AGENTS: &[&str] = &[
    "ralphx-general-explorer",
    "ralphx-general-worker",
    "ralphx-chat-task",
    "ralphx-chat-project",
    "ralphx-ideation",
    "ralphx-ideation-readonly",
    "ralphx-review-chat",
    "ralphx-review-history",
    "ralphx-execution-worker",
    "ralphx-execution-coder",
    "ralphx-execution-reviewer",
    "ralphx-execution-merger",
    "ralphx-qa-prep",
    "ralphx-qa-executor",
    "ralphx-research-deep-researcher",
    "ralphx-workspace-reviewer",
];
const CLAUDE_DELEGATION_GUIDE_AGENTS: &[&str] =
    &["ralphx-pr-reviewer", "ralphx-workspace-reviewer"];
const CLAUDE_ONLY_CANONICAL_AGENTS: &[(&str, &str, &str)] =
    &[("ralphx-pr-reviewer", "pr_reviewer", "ralphx-pr-reviewer")];
const CLAUDE_ONLY_SHARED_PROMPT_AGENTS: &[&str] = &["ralphx-pr-reviewer"];

const CROSS_HARNESS_VERIFICATION_AGENTS: &[(&str, &str, &str)] = &[];

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
    (
        "ralphx-execution-branch-updater",
        "branch-updater",
        "branch-updater",
    ),
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
    ("ralphx-task-manager", "task_manager", "ralphx-task-manager"),
];

const CROSS_HARNESS_SUPPORT_AGENTS: &[(&str, &str, &str)] = &[
    (
        "ralphx-workspace-annotator",
        "workspace_annotator",
        "ralphx-workspace-annotator",
    ),
    (
        "ralphx-automation-judge",
        "automation-judge",
        "ralphx-automation-judge",
    ),
    (
        "ralphx-automation-plan-judge",
        "automation-plan-judge",
        "ralphx-automation-plan-judge",
    ),
    (
        "ralphx-automation-decomposition-verifier",
        "automation-decomposition-verifier",
        "ralphx-automation-decomposition-verifier",
    ),
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
        "ralphx-agent-workspace-pr-fixer",
        "agent_workspace_pr_fixer",
        "ralphx-agent-workspace-pr-fixer",
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

const PERSONA_EXTRACTOR_AGENTS: &[(&str, &str, &str)] = &[(
    "ralphx-persona-extractor",
    "persona_extractor",
    "ralphx-persona-extractor",
)];

const CROSS_HARNESS_READONLY_IDEATION_AGENTS: &[(&str, &str, &str)] = &[(
    "ralphx-ideation-readonly",
    "ideation_orchestrator_readonly",
    "ralphx-ideation-readonly",
)];

const CANONICAL_MCP_TOOL_OWNED_AGENTS: &[&str] = &[
    "ralphx-general-explorer",
    "ralphx-general-worker",
    "ralphx-agent-workspace-repair",
    "ralphx-agent-workspace-pr-fixer",
    "ralphx-pr-reviewer",
    "ralphx-workspace-reviewer",
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
    "ralphx-utility-session-namer",
    "ralphx-utility-pr-describer",
    "ralphx-utility-plan-complexity",
    "ralphx-automation-judge",
    "ralphx-automation-plan-judge",
    "ralphx-automation-decomposition-verifier",
    "ralphx-automation-setup",
    "ralphx-chat-task",
    "ralphx-chat-project",
    "ralphx-task-manager",
    "ralphx-review-chat",
    "ralphx-review-history",
    "ralphx-execution-orchestrator",
    "ralphx-project-analyzer",
    "ralphx-qa-prep",
    "ralphx-ideation-specialist-backend",
    "ralphx-ideation-specialist-frontend",
    "ralphx-ideation-specialist-infra",
    "ralphx-ideation-advocate",
    "ralphx-ideation-critic",
];

const CANONICAL_CODEX_RUNTIME_FEATURE_OWNED_AGENTS: &[&str] = &[
    "ralphx-general-explorer",
    "ralphx-automation-judge",
    "ralphx-automation-plan-judge",
    "ralphx-automation-decomposition-verifier",
    "ralphx-automation-setup",
    "ralphx-utility-pr-describer",
    "ralphx-utility-plan-complexity",
    "ralphx-qa-prep",
    "ralphx-ideation-specialist-backend",
    "ralphx-ideation-specialist-frontend",
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
    ("ralphx-pr-reviewer", &["Write", "Edit", "NotebookEdit"]),
    (
        "ralphx-workspace-reviewer",
        &["Write", "Edit", "NotebookEdit", "Bash"],
    ),
    (
        "ralphx-automation-setup",
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
    "ralphx-agent-workspace-pr-fixer",
    "ralphx-pr-reviewer",
    "ralphx-workspace-reviewer",
    "ralphx-automation-setup",
    "ralphx-automation-judge",
    "ralphx-automation-plan-judge",
    "ralphx-automation-decomposition-verifier",
    "ralphx-execution-worker",
    "ralphx-execution-coder",
    "ralphx-execution-merger",
    "ralphx-memory-maintainer",
    "ralphx-memory-capture",
    "ralphx-utility-session-namer",
    "ralphx-chat-task",
    "ralphx-chat-project",
    "ralphx-task-manager",
    "ralphx-review-chat",
    "ralphx-review-history",
    "ralphx-execution-orchestrator",
    "ralphx-project-analyzer",
    "ralphx-execution-reviewer",
    "ralphx-ideation",
    "ralphx-ideation-readonly",
    "ralphx-qa-executor",
    "ralphx-qa-prep",
    "ralphx-research-deep-researcher",
    "ralphx-ideation-specialist-backend",
    "ralphx-ideation-specialist-frontend",
    "ralphx-ideation-specialist-infra",
    "ralphx-ideation-advocate",
    "ralphx-ideation-critic",
];

const CANONICAL_CLAUDE_PERMISSION_MODE_OWNED_AGENTS: &[(&str, &str)] = &[
    ("ralphx-automation-setup", "default"),
    ("ralphx-general-worker", "acceptEdits"),
    ("ralphx-agent-workspace-repair", "acceptEdits"),
    ("ralphx-agent-workspace-pr-fixer", "acceptEdits"),
    ("ralphx-execution-worker", "acceptEdits"),
    ("ralphx-execution-coder", "acceptEdits"),
    ("ralphx-execution-merger", "acceptEdits"),
    ("ralphx-qa-executor", "acceptEdits"),
    ("ralphx-memory-maintainer", "acceptEdits"),
    ("ralphx-memory-capture", "acceptEdits"),
];

const CANONICAL_CLAUDE_MODEL_OWNED_AGENTS: &[(&str, &str)] = &[
    ("ralphx-general-explorer", "sonnet"),
    ("ralphx-general-worker", "sonnet"),
    ("ralphx-agent-workspace-repair", "opus"),
    ("ralphx-agent-workspace-pr-fixer", "opus"),
    ("ralphx-pr-reviewer", "sonnet"),
    ("ralphx-workspace-reviewer", "sonnet"),
    ("ralphx-automation-setup", "sonnet"),
    ("ralphx-automation-judge", "haiku"),
    ("ralphx-automation-decomposition-verifier", "haiku"),
    ("ralphx-utility-session-namer", "haiku"),
    ("ralphx-utility-plan-complexity", "haiku"),
    ("ralphx-chat-task", "sonnet"),
    ("ralphx-chat-project", "sonnet"),
    ("ralphx-task-manager", "sonnet"),
    ("ralphx-review-chat", "sonnet"),
    ("ralphx-review-history", "sonnet"),
    ("ralphx-execution-orchestrator", "opus"),
    ("ralphx-project-analyzer", "haiku"),
    ("ralphx-execution-worker", "sonnet"),
    ("ralphx-execution-coder", "sonnet"),
    ("ralphx-execution-reviewer", "sonnet"),
    ("ralphx-execution-merger", "opus"),
    ("ralphx-ideation", "opus"),
    ("ralphx-ideation-readonly", "opus"),
    ("ralphx-qa-prep", "sonnet"),
    ("ralphx-qa-executor", "sonnet"),
    ("ralphx-research-deep-researcher", "opus"),
    ("ralphx-ideation-specialist-backend", "opus"),
    ("ralphx-ideation-specialist-frontend", "opus"),
    ("ralphx-ideation-specialist-infra", "opus"),
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
    (
        "ralphx-agent-workspace-pr-fixer",
        "base_tools",
        &["Edit"],
        false,
    ),
    ("ralphx-pr-reviewer", "readonly_tools", &["Bash"], false),
    ("ralphx-workspace-reviewer", "readonly_tools", &[], false),
    ("ralphx-automation-setup", "readonly_tools", &[], false),
    ("ralphx-chat-task", "base_tools", &["Task"], false),
    ("ralphx-chat-project", "readonly_tools", &[], false),
    ("ralphx-task-manager", "readonly_tools", &[], false),
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
    ("ralphx-execution-reviewer", "readonly_tools", &[], false),
    ("ralphx-qa-prep", "base_tools", &["Task"], false),
    (
        "ralphx-qa-executor",
        "base_tools",
        &["Write", "Edit", "Task"],
        false,
    ),
    ("ralphx-execution-merger", "base_tools", &["Edit"], false),
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

    let workspace_reviewer = load_canonical_codex_metadata(&root, "ralphx-workspace-reviewer");
    assert_eq!(
        workspace_reviewer.runtime_features.get("shell_tool"),
        Some(&false),
        "workspace reviewer should disable Codex shell_tool declaratively"
    );

    let general_worker = load_canonical_codex_metadata(&root, "ralphx-general-worker");
    assert_eq!(
        general_worker.runtime_features.get("shell_tool"),
        Some(&true),
        "general worker should declare Codex metadata instead of falling back to default"
    );
}

#[test]
fn persona_extractor_canonical_definition_loads() {
    let definition = load_canonical_agent_definition(&project_root(), "ralphx-persona-extractor")
        .expect("persona extractor canonical definition should load");

    assert_eq!(definition.role, "persona_extractor");
    assert_eq!(
        definition.capabilities.mcp_tools,
        vec![
            "fs_read_file",
            "fs_list_dir",
            "fs_grep",
            "fs_glob",
            "ask_user_question",
            "save_persona_draft",
            "get_persona_draft",
        ]
    );
    assert!(
        definition.delegation.allowed_targets.is_empty(),
        "persona extractor must remain unspawnable until PersonaBuilder mode owns dispatch"
    );
}

#[test]
fn persona_extractor_prompts_exist_for_claude_and_codex() {
    let root = project_root();

    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        assert!(
            load_harness_agent_prompt(&root, "ralphx-persona-extractor", harness).is_some(),
            "persona extractor should provide an explicit {harness:?} prompt"
        );
    }
}

#[test]
fn persona_extractor_prompts_mention_only_live_tools() {
    let root = project_root();
    let granted_tools = [
        "fs_read_file",
        "fs_list_dir",
        "fs_grep",
        "fs_glob",
        "ask_user_question",
        "save_persona_draft",
        "get_persona_draft",
    ];
    let ungranted_tools = [
        "delegate_start",
        "delegate_wait",
        "delegate_cancel",
        "create_agent_task",
        "get_agent_task",
        "list_agent_tasks",
        "update_agent_task",
        "claim_agent_task",
        "complete_agent_task",
        "get_artifact",
        "create_plan_artifact",
        "update_plan_artifact",
        "edit_plan_artifact",
        "search_memories",
        "get_memory",
        "get_memories_for_paths",
        "get_session_plan",
        "get_task_context",
        "Read",
        "Grep",
        "Glob",
        "Bash",
        "Write",
        "Edit",
        "NotebookEdit",
    ];

    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        let prompt = load_harness_agent_prompt(&root, "ralphx-persona-extractor", harness)
            .expect("persona extractor prompt should exist");
        for granted in granted_tools {
            assert!(
                prompt.contains(granted),
                "persona extractor {harness:?} prompt should name granted tool {granted}"
            );
        }
        for ungranted in ungranted_tools {
            assert!(
                !prompt.contains(ungranted),
                "persona extractor {harness:?} prompt must not mention off-surface tool {ungranted}"
            );
        }
    }

    let claude_prompt = load_harness_agent_prompt(
        &root,
        "ralphx-persona-extractor",
        AgentPromptHarness::Claude,
    )
    .expect("persona extractor Claude prompt should exist");
    assert!(
        claude_prompt.contains("TaskList"),
        "Claude prompt should document its inert CLI filler"
    );

    let codex_prompt =
        load_harness_agent_prompt(&root, "ralphx-persona-extractor", AgentPromptHarness::Codex)
            .expect("persona extractor Codex prompt should exist");
    assert!(
        !codex_prompt.contains("TaskList"),
        "Codex prompt must not mention a Claude-only tool"
    );
}

#[test]
fn persona_extractor_prompt_requires_distilled_persona_output() {
    let root = project_root();

    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        let prompt = load_harness_agent_prompt(&root, "ralphx-persona-extractor", harness)
            .expect("persona extractor prompt should exist");
        for required_contract_text in [
            "SKILL.md",
            "name",
            "kind: persona",
            "description",
            "10KB",
            "150 lines",
            "archived",
            "slug-collision",
        ] {
            assert!(
                prompt.contains(required_contract_text),
                "persona extractor {harness:?} prompt should require {required_contract_text}"
            );
        }
    }
}

#[test]
fn persona_extractor_prompt_requires_in_conversation_persistence() {
    let root = project_root();

    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        let prompt = load_harness_agent_prompt(&root, "ralphx-persona-extractor", harness)
            .expect("persona extractor prompt should exist");
        for required_contract_text in [
            "one Persona lineage",
            "separate Persona Builder conversation",
            "`save_persona_draft` succeeds",
            "Markdown-only response is not successful completion",
            "Do not present paste-ready persona content",
            "Persona tab",
            "Approve persona",
        ] {
            assert!(
                prompt.contains(required_contract_text),
                "persona extractor {harness:?} prompt should require `{required_contract_text}`"
            );
        }
    }
}

#[test]
fn persona_extractor_codex_prompt_has_no_claude_syntax() {
    let prompt = load_harness_agent_prompt(
        &project_root(),
        "ralphx-persona-extractor",
        AgentPromptHarness::Codex,
    )
    .expect("persona extractor Codex prompt should exist");

    assert!(!prompt.starts_with("---"));
    for banned in [
        "Task(",
        "TaskCreate",
        "TaskUpdate",
        "TaskGet",
        "TaskOutput",
        "TaskStop",
        "TeamCreate",
        "TeamDelete",
        "SendMessage",
        "mcpServers",
        "mcp__ralphx__",
        "CLAUDE_PLUGIN_ROOT",
        "--append-system-prompt",
    ] {
        assert!(
            !prompt.contains(banned),
            "persona extractor Codex prompt must not contain Claude-only syntax `{banned}`"
        );
    }
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
fn project_chat_codex_surface_mixes_external_flow_tools_with_internal_agent_tasks() {
    let root = project_root();
    let metadata = load_canonical_codex_metadata(&root, "ralphx-chat-project");
    let prompt = load_harness_agent_prompt(&root, "ralphx-chat-project", AgentPromptHarness::Codex)
        .expect("missing codex prompt for ralphx-chat-project");

    assert_eq!(metadata.mcp_transport.as_deref(), Some("external"));
    assert!(
        metadata
            .mcp_tools
            .iter()
            .all(|tool| !tool.ends_with("_agent_task")),
        "agent-task tools should not be exposed through the external project-chat surface"
    );
    assert!(
        metadata
            .internal_mcp_tools
            .iter()
            .any(|tool| tool == "create_agent_task"),
        "project chat Codex should receive native agent-task tools from the internal sidecar"
    );
    assert!(
        prompt.contains("list_agent_tasks"),
        "project chat Codex prompt should mention internal native agent-task tools"
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
fn project_chat_claude_surface_mixes_external_flow_tools_with_internal_agent_tasks() {
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
            .all(|tool| !tool.ends_with("_agent_task")),
        "agent-task tools should not be exposed through the external project-chat surface"
    );
    assert!(
        metadata
            .internal_mcp_tools
            .iter()
            .any(|tool| tool == "create_agent_task"),
        "project chat Claude should receive native agent-task tools from the internal sidecar"
    );
    assert!(
        prompt.contains("list_agent_tasks"),
        "project chat Claude prompt should mention internal native agent-task tools"
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
fn plan_mode_uses_orchestrator_prompt_with_constrained_plan_tools() {
    let root = project_root();
    let base_definition = load_canonical_agent_definition(&root, "ralphx-ideation")
        .expect("missing base definition for ralphx-ideation");
    let definition =
        load_canonical_agent_definition_for_profile(&root, "ralphx-ideation", Some("plan"))
            .expect("missing plan profile for ralphx-ideation");
    let runtime_config = get_agent_config_for_profile("ralphx-ideation", Some("plan"))
        .expect("missing runtime plan profile for ralphx-ideation");
    let preapproved_tools = get_preapproved_tools_for_profile("ralphx-ideation", Some("plan"))
        .expect("missing preapproved tools for ralphx-ideation plan profile");
    let claude_metadata =
        load_canonical_claude_metadata_for_profile(&root, "ralphx-ideation", Some("plan"));
    let codex_metadata =
        load_canonical_codex_metadata_for_profile(&root, "ralphx-ideation", Some("plan"));
    let claude_prompt = load_harness_agent_prompt_for_profile(
        &root,
        "ralphx-ideation",
        AgentPromptHarness::Claude,
        Some("plan"),
    )
    .expect("missing claude prompt for ralphx-ideation plan profile");
    let codex_prompt = load_harness_agent_prompt_for_profile(
        &root,
        "ralphx-ideation",
        AgentPromptHarness::Codex,
        Some("plan"),
    )
    .expect("missing codex prompt for ralphx-ideation plan profile");
    let runtime_profile_context =
        render_agent_runtime_profile_context(&root, "ralphx-ideation", Some("plan"))
            .expect("missing runtime profile context");

    assert_eq!(
        definition.capabilities.mcp_tools, runtime_config.allowed_mcp_tools,
        "Plan profile runtime MCP grants should be sourced from canonical capabilities"
    );
    assert_eq!(definition.role, "plan_chat");
    assert_eq!(
        definition.delegation.allowed_targets,
        vec!["ralphx-general-explorer".to_string()]
    );
    assert_eq!(
        claude_metadata.tools.and_then(|tools| tools.extends),
        Some("readonly_tools".to_string())
    );
    assert!(
        !preapproved_tools.contains("Task(Plan)"),
        "Plan profile should clear the base ideation Task preapproval"
    );
    assert_eq!(
        codex_metadata.runtime_features.get("shell_tool"),
        Some(&false)
    );
    assert!(runtime_profile_context.contains("<agent_runtime_profile>"));
    assert!(runtime_profile_context.contains("<profile_slug>plan</profile_slug>"));
    assert!(runtime_profile_context.contains("<profile_role>plan_chat</profile_role>"));
    assert!(
        render_agent_runtime_profile_context(&root, "ralphx-ideation", None).is_none(),
        "default launches should not receive profile context"
    );
    assert!(
        base_definition
            .capabilities
            .mcp_tools
            .iter()
            .any(|tool| tool == "create_child_session"),
        "Standalone ralphx-ideation should keep child sessions for the opt-in Ideation UI"
    );

    for required_tool in [
        "ask_user_question",
        "create_plan_artifact",
        "update_plan_artifact",
        "edit_plan_artifact",
        "get_session_plan",
        "get_plan_verification",
        "complete_plan_verification",
        "delegate_start",
        "delegate_wait",
        "delegate_cancel",
    ] {
        assert!(
            definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == required_tool),
            "Plan profile canonical surface should include {required_tool}"
        );
        assert!(
            runtime_config
                .allowed_mcp_tools
                .iter()
                .any(|tool| tool == required_tool),
            "Plan profile runtime surface should include {required_tool}"
        );
    }

    for forbidden_tool in [
        "create_task_proposal",
        "finalize_proposals",
        "migrate_proposals",
        "v1_start_ideation",
        "create_child_session",
        "create_followup_session",
        "create_followup_agent_conversation",
    ] {
        assert!(
            !definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == forbidden_tool),
            "Plan profile canonical surface must not expose {forbidden_tool}"
        );
        assert!(
            !runtime_config
                .allowed_mcp_tools
                .iter()
                .any(|tool| tool == forbidden_tool),
            "Plan profile runtime surface must not expose {forbidden_tool}"
        );
    }

    for prompt in [claude_prompt, codex_prompt] {
        assert!(prompt.contains("Agent Conversation Plan Mode"));
        assert!(prompt.contains("## RalphX Delegation Policy (AUTO-GENERATED)"));
        assert!(prompt.contains("`ralphx-general-explorer`"));
        assert!(prompt.contains("<plan_mode_context>"));
        assert!(prompt.contains("<planning_session_id>"));
        assert!(prompt.contains("do not invent a session id"));
        assert!(prompt.contains("ask_user_question"));
        assert!(prompt.contains("Create or update exactly one linked plan bundle"));
        assert!(prompt.contains("Call `get_session_plan` first"));
        assert!(prompt.contains("clicks the Plan-mode UI action `Approve Plan`"));
        assert!(prompt.contains("approval is backend/UI-owned"));
        assert!(prompt.contains("Agent-owned unknowns are facts"));
        assert!(prompt.contains("User-owned decisions are product, scope, priority"));
        assert!(prompt.contains("do not ask it only in prose"));
        assert!(prompt.contains("Separate evidence from inference"));
        assert!(prompt.contains("`## Data / State`"));
        assert!(prompt.contains("`## Agent And MCP Surface`"));
        assert!(prompt.contains("`## UI / UX`"));
        assert!(prompt.contains("`## Progression Scenarios`"));
        assert!(prompt.contains("Do not end a normal chat reply with a user-facing question"));
        assert!(prompt.contains("Do not paste the full plan into chat"));
        assert!(prompt.contains("Do not expose raw tool names"));
        assert!(prompt.contains("do not park blocking user-owned decisions there"));
        assert!(prompt.contains("Do not create task proposals"));
        assert!(prompt.contains("`Implement Plan` action"));
        assert!(
            !prompt.contains("create_child_session"),
            "Plan profile prompt must not mention off-surface child-session tools"
        );
        assert!(
            !prompt.contains("create_followup_session"),
            "Plan profile prompt must not mention off-surface follow-up tools"
        );
        assert!(
            !prompt.contains("create_followup_agent_conversation"),
            "Plan profile prompt must not mention off-surface Agent-conversation follow-up tools"
        );
        assert!(!prompt.contains("<agent_task_ledger_contract>"));
    }
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
fn chat_and_edit_agents_expose_plan_mode_proposal_contract() {
    let root = project_root();

    for (agent_name, mode_name) in [
        ("ralphx-general-explorer", "Chat mode"),
        ("ralphx-general-worker", "Edit mode"),
    ] {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("expected canonical definition for {agent_name}"));
        assert!(
            definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == "propose_plan_mode"),
            "{agent_name} should receive propose_plan_mode"
        );

        for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
            let prompt = load_harness_agent_prompt(&root, agent_name, harness)
                .unwrap_or_else(|| panic!("missing {harness:?} prompt for {agent_name}"));
            assert!(
                prompt.contains("Plan-mode proposal gate"),
                "{agent_name} {harness:?} prompt should declare the plan proposal gate"
            );
            assert!(
                prompt.contains(mode_name),
                "{agent_name} {harness:?} prompt should scope the gate to {mode_name}"
            );
            assert!(
                prompt.contains("call `propose_plan_mode` first"),
                "{agent_name} {harness:?} prompt should make propose_plan_mode the first step"
            );
            assert!(
                prompt.contains("broad planning, product-surface, architecture, workflow design"),
                "{agent_name} {harness:?} prompt should define the trigger shape"
            );
            assert!(
                prompt.contains("quick answer"),
                "{agent_name} {harness:?} prompt should preserve a quick-answer escape hatch"
            );
        }
    }
}

#[test]
fn pr_describer_surfaces_share_preserve_or_patch_submit_contract() {
    let root = project_root();
    let definition = load_canonical_agent_definition(&root, "ralphx-utility-pr-describer")
        .expect("expected canonical PR describer definition");

    assert_eq!(
        definition.capabilities.mcp_tools,
        vec!["submit_agent_workspace_pr_description".to_string()]
    );
    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        let prompt = load_harness_agent_prompt(&root, "ralphx-utility-pr-describer", harness)
            .expect("expected PR describer prompt");
        assert!(
            prompt.contains("submit_agent_workspace_pr_description"),
            "PR describer {harness:?} prompt should expose the submit contract"
        );
        assert!(
            prompt.contains("`decision: preserve`") && prompt.contains("`decision: patch`"),
            "PR describer {harness:?} prompt should expose preserve and patch decisions"
        );
        assert!(
            prompt.contains("empty final answer and no tool call")
                && prompt.contains("Every patch and every new PR must call"),
            "PR describer {harness:?} prompt should allow only existing-PR silent preserve"
        );
        assert!(
            prompt.contains("Explanatory final prose is not a preserve decision"),
            "PR describer {harness:?} prompt should reject prose as implicit preserve"
        );
        assert!(
            prompt.contains("untrusted evidence"),
            "PR describer {harness:?} prompt should identify remote metadata as untrusted"
        );
        assert!(
            prompt.contains("patch_allowed=\"true\"")
                && prompt.contains("managed_suffix_preserved=\"true\"")
                && prompt.contains("max_output_chars"),
            "PR describer {harness:?} prompt should expose the editable-body capability contract"
        );
        assert!(
            prompt.contains("never include or reconstruct"),
            "PR describer {harness:?} prompt should exclude preserved managed content"
        );
        assert!(
            !prompt.contains("mcp__ralphx__"),
            "PR describer {harness:?} prompt should use surface-local tool names"
        );
        assert!(
            !prompt.contains("truncated"),
            "PR describer {harness:?} prompt should not tell helpers to expose bounded-context truncation"
        );
    }

    let metadata = load_canonical_codex_metadata(&root, "ralphx-utility-pr-describer");
    assert_eq!(metadata.runtime_features.get("shell_tool"), Some(&false));
}

#[test]
fn workspace_reviewer_codex_surface_uses_shared_prompt_and_review_tools() {
    let root = project_root();
    let definition = load_canonical_agent_definition(&root, "ralphx-workspace-reviewer")
        .expect("expected canonical workspace reviewer definition");
    let prompt = load_harness_agent_prompt(
        &root,
        "ralphx-workspace-reviewer",
        AgentPromptHarness::Codex,
    )
    .expect("expected workspace reviewer Codex prompt");
    let metadata = load_canonical_codex_metadata(&root, "ralphx-workspace-reviewer");

    for required_tool in [
        "fs_read_file",
        "fs_list_dir",
        "fs_grep",
        "fs_glob",
        "get_workspace_review_context",
        "write_workspace_review_artifact",
        "complete_workspace_review_run",
        "delegate_start",
        "delegate_wait",
        "delegate_cancel",
    ] {
        assert!(
            definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == required_tool),
            "workspace reviewer canonical surface should include {required_tool}"
        );
        assert!(
            metadata.mcp_tools.iter().any(|tool| tool == required_tool),
            "workspace reviewer Codex surface should include {required_tool}"
        );
        assert!(
            prompt.contains(required_tool),
            "workspace reviewer Codex prompt should mention {required_tool}"
        );
    }

    assert!(
        !prompt.contains("mcp__ralphx__"),
        "Codex workspace reviewer prompt should not use Claude-style MCP names"
    );
    assert!(
        prompt.contains("Artifact freshness is historical context")
            && prompt.contains("can_mutate_review_state=true"),
        "workspace reviewer prompt should make active-run authority independent from prior artifact freshness"
    );
    for marker in [
        "Cost of doing nothing",
        "Fold In",
        "Backlog",
        "Informational",
        "Behavior Changes Beyond Stated Goal",
        "**Disposition:**",
        "review_fixer_status",
        "review_fixer_cycle_count",
    ] {
        assert!(
            prompt.contains(marker),
            "workspace reviewer prompt should carry the finding classification contract marker {marker}"
        );
    }
    for removed_tool in [
        "get_agent_task",
        "list_agent_tasks",
        "search_memories",
        "get_memory",
        "get_memories_for_paths",
        // Hunk annotations moved to the background ralphx-workspace-annotator; the reviewer must
        // not hold the tool, or its run tail reacquires the work the annotator now owns.
        "write_workspace_review_hunk_annotations",
    ] {
        assert!(
            !definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == removed_tool),
            "workspace reviewer should not retain unrelated MCP tool {removed_tool}"
        );
        assert!(
            !metadata.mcp_tools.iter().any(|tool| tool == removed_tool),
            "workspace reviewer Codex surface should not retain unrelated MCP tool {removed_tool}"
        );
        assert!(
            !prompt.contains(removed_tool),
            "workspace reviewer prompt should not instruct use of {removed_tool}"
        );
    }
    // The reviewer performs exactly one artifact write, at the end, always carrying a typed
    // outcome. A provisional early write would make `previous_review` self-referential and would
    // leave a half-finished pair the degraded-settlement path must never trust.
    assert_eq!(
        prompt.matches("Call `write_workspace_review_artifact`").count(),
        1,
        "workspace reviewer prompt should contain exactly one artifact-write step"
    );
    assert!(
        !prompt.to_lowercase().contains("provisional"),
        "workspace reviewer prompt should no longer instruct a provisional artifact write"
    );
    assert!(
        prompt.contains("`outcome` matching your disposition line")
            && prompt.contains("blocking_summary"),
        "workspace reviewer prompt should require the typed outcome on its single artifact write"
    );
    assert_eq!(
        metadata.runtime_features.get("shell_tool"),
        Some(&false),
        "workspace reviewer should use review_packet and bounded filesystem tools instead of Codex shell"
    );
}

/// The annotator writes reading aids for a settled review. It must hold the annotation tool and
/// nothing that could reach the review gate.
#[test]
fn workspace_annotator_surface_is_annotation_only() {
    let root = project_root();
    let definition = load_canonical_agent_definition(&root, "ralphx-workspace-annotator")
        .expect("expected canonical workspace annotator definition");
    let prompt = load_harness_agent_prompt(
        &root,
        "ralphx-workspace-annotator",
        AgentPromptHarness::Codex,
    )
    .expect("expected workspace annotator Codex prompt");
    let metadata = load_canonical_codex_metadata(&root, "ralphx-workspace-annotator");

    for required_tool in [
        "get_workspace_review_context",
        "list_workspace_review_files",
        "get_workspace_review_diff_page",
        "write_workspace_review_hunk_annotations",
    ] {
        assert!(
            definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == required_tool),
            "workspace annotator canonical surface should include {required_tool}"
        );
        assert!(
            metadata.mcp_tools.iter().any(|tool| tool == required_tool),
            "workspace annotator Codex surface should include {required_tool}"
        );
        assert!(
            prompt.contains(required_tool),
            "workspace annotator prompt should mention {required_tool}"
        );
    }

    for denied_tool in [
        "write_workspace_review_artifact",
        "complete_workspace_review_run",
        "delegate_start",
    ] {
        assert!(
            !definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == denied_tool),
            "workspace annotator must not hold gate-mutating tool {denied_tool}"
        );
        assert!(
            !metadata.mcp_tools.iter().any(|tool| tool == denied_tool),
            "workspace annotator Codex surface must not hold {denied_tool}"
        );
    }

    assert!(
        !prompt.contains("mcp__ralphx__"),
        "Codex workspace annotator prompt should not use Claude-style MCP names"
    );
    assert_eq!(
        metadata.runtime_features.get("shell_tool"),
        Some(&false),
        "workspace annotator should read diffs through review tools, not a Codex shell"
    );
}

#[test]
fn pr_reviewer_prompt_carries_the_finding_classification_contract() {
    let root = project_root();
    let prompt = load_harness_agent_prompt(&root, "ralphx-pr-reviewer", AgentPromptHarness::Claude)
        .expect("expected PR reviewer Claude prompt");

    for marker in [
        "Cost of doing nothing",
        "Fold In",
        "Backlog",
        "Informational",
        "Behavior Changes Beyond Stated Goal",
        "**Disposition:**",
        "Blocking findings map to `Request Changes`",
        "Fold In findings with no Blocking findings map to `Comment / No Action`",
        "No requested work maps to `Approve PR`",
        "maps to `Blocked`",
    ] {
        assert!(
            prompt.contains(marker),
            "PR reviewer prompt should carry the finding classification contract marker {marker}"
        );
    }
}

#[test]
fn plan_complexity_codex_surface_uses_shared_prompt_and_submit_tool() {
    let root = project_root();
    let definition = load_canonical_agent_definition(&root, "ralphx-utility-plan-complexity")
        .expect("expected canonical plan complexity definition");
    let prompt = load_harness_agent_prompt(
        &root,
        "ralphx-utility-plan-complexity",
        AgentPromptHarness::Codex,
    )
    .expect("expected plan complexity Codex prompt");
    let metadata = load_canonical_codex_metadata(&root, "ralphx-utility-plan-complexity");

    assert_eq!(
        definition.capabilities.mcp_tools,
        vec!["submit_plan_complexity_assessment".to_string()]
    );
    assert!(
        prompt.contains("submit_plan_complexity_assessment"),
        "Codex plan complexity prompt should expose the submit contract"
    );
    assert!(
        !prompt.contains("mcp__ralphx__"),
        "Codex plan complexity prompt should not use Claude-style MCP names"
    );
    assert_eq!(metadata.runtime_features.get("shell_tool"), Some(&false));
}

#[test]
fn automation_judge_surface_is_output_only_and_zero_tool() {
    let root = project_root();
    let definition = load_canonical_agent_definition(&root, "ralphx-automation-judge")
        .expect("expected canonical automation judge definition");
    let claude_prompt =
        load_harness_agent_prompt(&root, "ralphx-automation-judge", AgentPromptHarness::Claude)
            .expect("expected automation judge Claude prompt");
    let codex_prompt =
        load_harness_agent_prompt(&root, "ralphx-automation-judge", AgentPromptHarness::Codex)
            .expect("expected automation judge Codex prompt");
    let codex_metadata = load_canonical_codex_metadata(&root, "ralphx-automation-judge");
    let runtime_config = get_agent_config("ralphx-automation-judge")
        .expect("expected runtime config for automation judge");

    assert_eq!(definition.capabilities.mcp_tools, Vec::<String>::new());
    assert_eq!(runtime_config.allowed_mcp_tools, Vec::<String>::new());
    assert!(
        runtime_config.mcp_only,
        "automation judge should not receive general Claude tools"
    );
    assert_eq!(
        codex_metadata.runtime_features.get("shell_tool"),
        Some(&false)
    );

    for prompt in [&claude_prompt, &codex_prompt] {
        assert!(
            prompt.contains("Return only one JSON object"),
            "judge prompt should require an output-only JSON verdict"
        );
        assert!(
            prompt.contains("\"updatedItemStatuses\""),
            "judge prompt should declare item status updates"
        );
        assert!(
            prompt.contains("\"goalItemsProposal\""),
            "judge prompt should declare structural goal-item proposals"
        );
        assert!(
            prompt.contains("\"nextBaseBranch\""),
            "judge prompt should declare next base selection"
        );
        assert!(
            !prompt.contains("mcp__ralphx__"),
            "judge prompt must not mention Claude-style MCP names"
        );
        assert!(
            !prompt.contains("MCP Tools Available"),
            "judge prompt must not describe an unavailable MCP surface"
        );
    }
}

#[test]
fn automation_decomposition_verifier_surface_is_output_only_and_zero_tool() {
    let root = project_root();
    let agent_name = "ralphx-automation-decomposition-verifier";
    let definition = load_canonical_agent_definition(&root, agent_name)
        .expect("expected canonical automation decomposition verifier definition");
    let claude_prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Claude)
        .expect("expected decomposition verifier Claude prompt");
    let codex_prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
        .expect("expected decomposition verifier Codex prompt");
    let codex_metadata = load_canonical_codex_metadata(&root, agent_name);
    let runtime_config =
        get_agent_config(agent_name).expect("expected runtime config for decomposition verifier");

    assert_eq!(definition.capabilities.mcp_tools, Vec::<String>::new());
    assert_eq!(runtime_config.allowed_mcp_tools, Vec::<String>::new());
    assert!(runtime_config.mcp_only);
    assert_eq!(
        codex_metadata.runtime_features.get("shell_tool"),
        Some(&false)
    );
    for prompt in [&claude_prompt, &codex_prompt] {
        assert!(prompt.contains("Return exactly one JSON object"));
        assert!(prompt.contains("automatic plan approval"));
        assert!(!prompt.contains("mcp__ralphx__"));
        assert!(!prompt.contains("MCP Tools Available"));
    }
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
    agents.extend_from_slice(PERSONA_EXTRACTOR_AGENTS);
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
            "ralphx-utility-session-namer"
                | "ralphx-utility-pr-describer"
                | "ralphx-utility-plan-complexity"
                | "ralphx-workspace-reviewer"
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

    for (agent_name, _, _) in CLAUDE_ONLY_CANONICAL_AGENTS {
        let claude_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Claude);
        let codex_path =
            resolve_harness_agent_prompt_path(&root, agent_name, AgentPromptHarness::Codex);
        assert!(
            claude_path.is_some(),
            "expected claude prompt path for {agent_name}"
        );

        if CLAUDE_ONLY_SHARED_PROMPT_AGENTS.contains(agent_name) {
            assert!(
                codex_path.as_ref().is_some_and(
                    |path| path.ends_with(format!("agents/{agent_name}/shared/prompt.md"))
                ),
                "expected {agent_name} codex path lookup to resolve through shared/prompt.md"
            );
        } else {
            assert!(
                codex_path.is_none(),
                "{agent_name} should remain without a codex prompt path until one exists"
            );
        }
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
fn branch_updater_uses_one_shared_prompt_for_both_harnesses() {
    let root = project_root();
    let expected_suffix = "agents/ralphx-execution-branch-updater/shared/prompt.md";
    let mut loaded_prompts = Vec::new();

    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        let path =
            resolve_harness_agent_prompt_path(&root, "ralphx-execution-branch-updater", harness)
                .unwrap_or_else(|| panic!("expected branch-updater prompt for {harness:?}"));

        assert!(
            path.ends_with(expected_suffix),
            "expected {harness:?} branch-updater prompt to resolve through shared/prompt.md, got {}",
            path.display()
        );
        loaded_prompts.push(
            load_harness_agent_prompt(&root, "ralphx-execution-branch-updater", harness)
                .unwrap_or_else(|| panic!("expected loaded branch-updater prompt for {harness:?}")),
        );
    }

    assert_eq!(loaded_prompts[0], loaded_prompts[1]);
    assert!(loaded_prompts[0].contains("get_branch_update_context(task_id)"));
    assert!(loaded_prompts[0].contains("complete_branch_update(task_id)"));
    assert!(loaded_prompts[0].contains("backend owns those mutations"));
}

#[test]
fn legacy_agent_aliases_resolve_into_canonical_catalog_entries() {
    let root = project_root();
    let cases = [
        ("orchestrator-ideation", "ralphx-ideation"),
        ("ralphx-worker", "ralphx-execution-worker"),
        ("session-namer", "ralphx-utility-session-namer"),
        ("pr-describer", "ralphx-utility-pr-describer"),
        ("plan-complexity", "ralphx-utility-plan-complexity"),
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
fn ideation_prompts_preserve_model_native_verification_without_legacy_topology() {
    let root = project_root();
    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        let prompt = load_harness_agent_prompt(&root, "ralphx-ideation", harness)
            .unwrap_or_else(|| panic!("missing {harness:?} prompt for ralphx-ideation"));
        assert!(prompt.contains("backend-started Verify Plan"));
        assert!(prompt.contains("complete_plan_verification"));
        for retired in [
            "ralphx-plan-verifier",
            "create_child_session(purpose: \"verification\")",
            "stop_verification",
            "revert_and_skip",
            "max_rounds",
        ] {
            assert!(
                !prompt.contains(retired),
                "{harness:?} ideation prompt still contains retired verification contract {retired}"
            );
        }
    }
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
            prompt.contains("delegate_park"),
            "codex prompt for {agent_name} should mention delegate_park"
        );
        assert!(
            prompt.contains("delegate_cancel"),
            "codex prompt for {agent_name} should mention delegate_cancel"
        );
    }

    for agent_name in CLAUDE_DELEGATION_GUIDE_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Claude)
            .unwrap_or_else(|| panic!("missing claude prompt for {agent_name}"));
        assert!(
            prompt.contains("delegate_start"),
            "claude prompt for {agent_name} should mention delegate_start"
        );
        assert!(
            prompt.contains("delegate_wait"),
            "claude prompt for {agent_name} should mention delegate_wait"
        );
        assert!(
            prompt.contains("delegate_park"),
            "claude prompt for {agent_name} should mention delegate_park"
        );
        assert!(
            prompt.contains("delegate_cancel"),
            "claude prompt for {agent_name} should mention delegate_cancel"
        );
    }
}

#[test]
fn canonical_delegation_policy_appendix_is_injected_only_for_delegating_agents() {
    let root = project_root();
    let required_waiting_clauses = [
        "For short waits, call `delegate_wait` once with `wait_timeout_ms`; the backend blocks until a delegate settles. Do not spin on repeated immediate `delegate_wait` calls.",
        "For long waits, call `delegate_park` with the outstanding job ids and then END YOUR TURN. RalphX resumes you automatically when the wake condition is met, on failure, or at the deadline.",
        "Parking is not abandonment: state what you are waiting for before ending the turn.",
    ];
    let required_coordinator_clauses = [
        "### Delegated Implementation Coordination",
        "otherwise do not direct mutation or validation",
        "Retain the full objective, integration decisions, integrated review, and final verification",
        "bounded, dependency-aware slices",
        "outcome, exclusive writable files or modules, established seam, required behavior, prohibited scope, acceptance criteria, permitted validation, and dependencies",
        "Exploration may cover owned files and immediate dependencies needed for safe implementation",
        "disjoint mutation sets, including generated artifacts, and disjoint command resource sets",
        "one serialized heavyweight-validation lane",
        "behavioral tests and only permitted cheap or local checks",
        "Directly verify suspected defects before a narrow, evidence-backed revision",
        "review the integrated workspace and run final focused validation once after revisions settle",
        "Do not publish unless the user requested it",
        "Repository and profile rules remain authoritative for exact commands, cleanup, resource conflicts, and stricter permissions",
    ];

    for agent_name in CODEX_DELEGATION_GUIDE_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        assert!(
            prompt.contains("## RalphX Delegation Policy (AUTO-GENERATED)"),
            "delegating codex prompt for {agent_name} should include the generated delegation appendix"
        );
        for clause in required_waiting_clauses {
            assert!(
                prompt.contains(clause),
                "delegating codex prompt for {agent_name} should include waiting clause: {clause}"
            );
        }
        for clause in required_coordinator_clauses {
            assert!(
                prompt.contains(clause),
                "delegating codex prompt for {agent_name} should include coordinator clause: {clause}"
            );
        }
    }

    for agent_name in CLAUDE_DELEGATION_GUIDE_AGENTS {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Claude)
            .unwrap_or_else(|| panic!("missing claude prompt for {agent_name}"));
        assert!(
            prompt.contains("## RalphX Delegation Policy (AUTO-GENERATED)"),
            "delegating claude prompt for {agent_name} should include the generated delegation appendix"
        );
        for clause in required_waiting_clauses {
            assert!(
                prompt.contains(clause),
                "delegating claude prompt for {agent_name} should include waiting clause: {clause}"
            );
        }
        for clause in required_coordinator_clauses {
            assert!(
                prompt.contains(clause),
                "delegating claude prompt for {agent_name} should include coordinator clause: {clause}"
            );
        }
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
    assert!(
        !session_namer.contains("### Delegated Implementation Coordination"),
        "non-delegating codex prompt should not include coordinator guidance"
    );
}

#[test]
fn canonical_agent_task_appendix_is_injected_only_for_agent_task_agents() {
    let root = project_root();

    for agent_name in [
        "ralphx-chat-task",
        "ralphx-chat-project",
        "ralphx-ideation",
        "ralphx-execution-worker",
        "ralphx-execution-coder",
        "ralphx-general-explorer",
        "ralphx-general-worker",
    ] {
        let prompt = load_harness_agent_prompt(&root, agent_name, AgentPromptHarness::Codex)
            .unwrap_or_else(|| panic!("missing codex prompt for {agent_name}"));
        assert!(
            prompt.contains("<agent_task_ledger_contract>"),
            "prompt for {agent_name} should include the generated XML-style agent task contract"
        );
        assert!(
            prompt.contains("list_agent_tasks"),
            "prompt for {agent_name} should mention live agent task tools"
        );
        assert!(
            prompt.contains("treat it as the authoritative ledger snapshot for this turn"),
            "prompt for {agent_name} should trust the injected runtime-state ledger snapshot"
        );
        assert!(
            prompt.contains("`state=\"empty\"` means no open or active tasks exist"),
            "prompt for {agent_name} should define the explicit empty-ledger marker"
        );
        assert!(
            prompt.contains("Call `list_agent_tasks` only when no `<task_ledger>` block is present"),
            "prompt for {agent_name} should scope list calls to missing/unavailable/stale snapshots"
        );
        assert!(
            prompt.contains("the block reports `state=\"unavailable\"`"),
            "prompt for {agent_name} should treat unavailable as unknown rather than empty"
        );
        assert!(
            prompt.contains(
                "create concrete tasks for each independent work item before performing search, read, shell, edit, or other tool-backed work"
            ),
            "prompt for {agent_name} should hard-gate required work behind task creation"
        );
        assert!(
            prompt.contains("trust the mutation tool results over the turn-start snapshot"),
            "prompt for {agent_name} should prefer fresh mutation results over the snapshot"
        );
        assert!(
            !prompt.contains("Before search, read, edit, shell, or other tool-backed work"),
            "prompt for {agent_name} should no longer mandate a redundant ledger read"
        );
        assert!(
            prompt.contains("Read-only audits, multi-file investigations, prompt-surface reviews"),
            "prompt for {agent_name} should classify read-only investigation as tool-backed work"
        );
        assert!(
            prompt.contains("For simple conversational turns or simple questions"),
            "prompt for {agent_name} should include the simple-chat exception"
        );
        assert!(
            prompt.contains("<breakdown_rules>"),
            "prompt for {agent_name} should include explicit breakdown rules"
        );
        assert!(
            prompt.contains("For user-provided numbered audit, check, or investigation lists"),
            "prompt for {agent_name} should require task splitting for numbered audits"
        );
        assert!(
            prompt.contains(
                "A single umbrella task is acceptable only for a single-question investigation"
            ),
            "prompt for {agent_name} should restrict umbrella tasks to non-independent work"
        );
        assert!(
            prompt.contains(
                "Do not claim, activate, or complete a ledger with only one meaningful task"
            ),
            "prompt for {agent_name} should explain the single-task ledger progression guard"
        );
        assert!(
            prompt.contains(
                "mark that accidental task `dropped` before continuing without the ledger"
            ),
            "prompt for {agent_name} should explain single-step cleanup"
        );
        assert!(
            prompt.contains("use `state=dropped` to clean up an accidental single-task ledger"),
            "prompt for {agent_name} should tie cleanup to the live update tool"
        );
        assert!(
            prompt.contains("Do not create one umbrella task for multiple independent fixes, checks, audit items, or investigation streams"),
            "prompt for {agent_name} should forbid umbrella tasks for independent work"
        );
        assert!(
            prompt.contains(
                "For two or more requested fixes, checks, audit items, or investigation streams"
            ),
            "prompt for {agent_name} should require one task per independent requested fix"
        );
        assert!(
            prompt.contains("<state_rules>"),
            "prompt for {agent_name} should include task state rules"
        );
        assert!(
            prompt.contains("<examples>"),
            "prompt for {agent_name} should include original usage examples"
        );
        assert!(
            prompt.contains("provider-spawn bug requires checking logs"),
            "prompt for {agent_name} should include RalphX-specific examples"
        );
        assert!(
            prompt.contains("three-part read-only audit covering Codex prompt injection"),
            "prompt for {agent_name} should include a read-only audit splitting example"
        );
        assert!(
            !prompt.contains("### When To Use The Ledger"),
            "prompt for {agent_name} should not use the legacy markdown ledger headings"
        );
    }

    let claude_general_worker =
        load_harness_agent_prompt(&root, "ralphx-general-worker", AgentPromptHarness::Claude)
            .expect("missing general worker claude prompt");
    assert!(
        claude_general_worker.contains("<agent_task_ledger_contract>"),
        "Claude prompt should include the XML-style agent task contract"
    );
    assert!(
        claude_general_worker.contains(
            "For two or more requested fixes, checks, audit items, or investigation streams"
        ),
        "Claude prompt should include the concrete multi-fix breakdown rule"
    );
    assert!(
        claude_general_worker.contains("<delegate_assignment_contract>")
            && claude_general_worker.contains("complete_delegate_assignment"),
        "delegated-capable prompts should distinguish local ledgers from bound assignments"
    );

    let session_namer = load_harness_agent_prompt(
        &root,
        "ralphx-utility-session-namer",
        AgentPromptHarness::Codex,
    )
    .expect("missing session namer codex prompt");
    assert!(
        !session_namer.contains("<agent_task_ledger_contract>"),
        "non-agent-task prompt should not include the generated agent task appendix"
    );
}

#[test]
fn generated_delegation_appendix_describes_general_explorer_usage_for_general_explorer_callers() {
    let root = project_root();

    for agent_name in [
        "ralphx-general-explorer",
        "ralphx-general-worker",
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
        "ralphx-workspace-reviewer",
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

    let pr_reviewer_prompt =
        load_harness_agent_prompt(&root, "ralphx-pr-reviewer", AgentPromptHarness::Claude)
            .expect("missing claude prompt for ralphx-pr-reviewer");
    assert!(
        pr_reviewer_prompt.contains("`ralphx-general-explorer`: read-only exploration delegate"),
        "delegation appendix for ralphx-pr-reviewer should describe the general explorer target"
    );
    assert!(
        pr_reviewer_prompt
            .contains("bounded file inspection, pattern search, or evidence gathering without edits"),
        "delegation appendix for ralphx-pr-reviewer should explain when to use the general explorer"
    );

    let workspace_reviewer_prompt = load_harness_agent_prompt(
        &root,
        "ralphx-workspace-reviewer",
        AgentPromptHarness::Claude,
    )
    .expect("missing claude prompt for ralphx-workspace-reviewer");
    assert!(
        workspace_reviewer_prompt
            .contains("`ralphx-general-explorer`: read-only exploration delegate"),
        "delegation appendix for ralphx-workspace-reviewer should describe the general explorer target"
    );
    assert!(
        workspace_reviewer_prompt
            .contains("bounded file inspection, pattern search, or evidence gathering without edits"),
        "delegation appendix for ralphx-workspace-reviewer should explain when to use the general explorer"
    );

    let general_worker_prompt =
        load_harness_agent_prompt(&root, "ralphx-general-worker", AgentPromptHarness::Codex)
            .expect("missing codex prompt for ralphx-general-worker");
    assert!(
        general_worker_prompt.contains("`ralphx-general-worker`: bounded implementation delegate"),
        "delegation appendix for ralphx-general-worker should describe its implementation delegate target"
    );
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
fn resolve_project_root_from_profiled_generated_plugin_dir_follows_runtime_symlinks() {
    let root = project_root();
    let temp = tempdir().expect("tempdir");
    let generated_plugin_dir = temp.path().join("generated/release/claude-plugin");
    fs::create_dir_all(&generated_plugin_dir).expect("create profiled generated plugin dir");
    symlink_dir(
        root.join("plugins/app/ralphx-mcp-server"),
        generated_plugin_dir.join("ralphx-mcp-server"),
    );

    assert_eq!(
        resolve_project_root_from_plugin_dir(&generated_plugin_dir),
        root,
        "profile-scoped generated plugin dirs should resolve through symlinked runtime entries back to the RalphX app resource root"
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

#[test]
fn execution_prompts_do_not_require_default_baseline_validation() {
    let root = project_root();
    let prompt_surfaces = [
        ("ralphx-execution-worker", AgentPromptHarness::Claude),
        ("ralphx-execution-worker", AgentPromptHarness::Codex),
        ("ralphx-execution-coder", AgentPromptHarness::Claude),
        ("ralphx-execution-coder", AgentPromptHarness::Codex),
    ];
    let forbidden_normal_flow_phrases = [
        "baseline/final validation commands",
        "selected baseline validation commands",
        "confirm clean baseline",
        "confirm a clean baseline",
        "establish clean baseline",
        "All validate commands must pass before writing code",
        "all validate commands must pass before writing code",
    ];

    for (agent_name, harness) in prompt_surfaces {
        let prompt = load_harness_agent_prompt(&root, agent_name, harness)
            .unwrap_or_else(|| panic!("missing {harness:?} prompt for {agent_name}"));
        for forbidden in forbidden_normal_flow_phrases {
            assert!(
                !prompt.contains(forbidden),
                "{agent_name} {harness:?} prompt must not require default baseline validation with `{forbidden}`"
            );
        }
        assert!(
            prompt.contains("get_project_analysis"),
            "{agent_name} {harness:?} prompt should still load validation commands"
        );
        assert!(
            prompt.contains("run_task_validation"),
            "{agent_name} {harness:?} prompt should still use backend-managed validation"
        );
        assert!(
            prompt.contains("final validation"),
            "{agent_name} {harness:?} prompt should still require final validation"
        );
    }
}

#[test]
fn ticket_attachment_surface_is_prompted_only_for_granted_execution_agents() {
    let root = project_root();
    let prompted_surfaces = [
        ("ralphx-execution-worker", AgentPromptHarness::Claude),
        ("ralphx-execution-worker", AgentPromptHarness::Codex),
        ("ralphx-execution-coder", AgentPromptHarness::Claude),
        ("ralphx-execution-coder", AgentPromptHarness::Codex),
    ];
    let attachment_tools = ["list_ticket_attachments", "fetch_ticket_attachment"];
    let forbidden_prompt_terms = [
        "direct-download URL",
        "download URL",
        "Bearer ",
        "bearer token",
        "credential",
        "cache path",
        "source handle",
    ];

    for (agent_name, harness) in prompted_surfaces {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("missing canonical definition for {agent_name}"));
        let prompt = load_harness_agent_prompt(&root, agent_name, harness)
            .unwrap_or_else(|| panic!("missing {harness:?} prompt for {agent_name}"));

        for tool in attachment_tools {
            assert!(
                definition
                    .capabilities
                    .mcp_tools
                    .iter()
                    .any(|candidate| candidate == tool),
                "{agent_name} canonical grant should include {tool}"
            );
            assert!(
                prompt.contains(tool),
                "{agent_name} {harness:?} prompt should describe {tool}"
            );
        }

        assert!(
            prompt.contains("opaque content pointers")
                && prompt.contains("untrusted external context"),
            "{agent_name} {harness:?} prompt should describe the safe attachment workflow"
        );
        for forbidden in forbidden_prompt_terms {
            assert!(
                !prompt.contains(forbidden),
                "{agent_name} {harness:?} prompt should not expose `{forbidden}` as an attachment contract"
            );
        }
    }
}

#[test]
fn ticket_attachment_surface_stays_off_unrelated_agent_prompts_and_grants() {
    let root = project_root();
    let unrelated_surfaces = [
        ("ralphx-ideation", AgentPromptHarness::Claude),
        ("ralphx-ideation", AgentPromptHarness::Codex),
        ("ralphx-execution-reviewer", AgentPromptHarness::Claude),
        ("ralphx-execution-reviewer", AgentPromptHarness::Codex),
        ("ralphx-execution-merger", AgentPromptHarness::Claude),
        ("ralphx-execution-merger", AgentPromptHarness::Codex),
        ("ralphx-general-worker", AgentPromptHarness::Claude),
        ("ralphx-general-worker", AgentPromptHarness::Codex),
    ];
    let attachment_tools = ["list_ticket_attachments", "fetch_ticket_attachment"];

    for (agent_name, harness) in unrelated_surfaces {
        let definition = load_canonical_agent_definition(&root, agent_name)
            .unwrap_or_else(|| panic!("missing canonical definition for {agent_name}"));
        let prompt = load_harness_agent_prompt(&root, agent_name, harness)
            .unwrap_or_else(|| panic!("missing {harness:?} prompt for {agent_name}"));

        for tool in attachment_tools {
            assert!(
                !definition
                    .capabilities
                    .mcp_tools
                    .iter()
                    .any(|candidate| candidate == tool),
                "{agent_name} canonical grant must not include {tool}"
            );
            assert!(
                !prompt.contains(tool),
                "{agent_name} {harness:?} prompt must not mention off-surface {tool}"
            );
        }
    }

    let plan_definition =
        load_canonical_agent_definition_for_profile(&root, "ralphx-ideation", Some("plan"))
            .expect("missing plan profile for ralphx-ideation");
    for tool in attachment_tools {
        assert!(
            !plan_definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|candidate| candidate == tool),
            "Plan profile canonical grant must not include {tool}"
        );
    }
}

#[test]
fn task_execution_agent_rule_documents_focused_validation_policy() {
    let root = project_root();
    let rule_doc = fs::read_to_string(root.join(".claude/rules/task-execution-agents.md"))
        .expect("read task execution agent rule");

    assert!(
        rule_doc.contains("Target-project instructions own command selection"),
        "task execution rule should preserve target-project validation ownership"
    );
    assert!(
        rule_doc.contains("narrowest relevant wave/final/re-execution evidence"),
        "task execution rule should require focused implementation-agent evidence"
    );
    assert!(
        rule_doc.contains("Execution agents do not run baseline validation by default"),
        "task execution rule should state the default pre-change validation policy"
    );
    assert!(
        rule_doc.contains("explicit diagnostic"),
        "task execution rule should preserve explicit diagnostic baseline use"
    );
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
    let promptless_metadata_only_agents = std::collections::BTreeSet::<String>::new();

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

// ─── Canonical profile-key contract ─────────────────────────────────────────
//
// RX-native Team mode shipped broken because `trusted_canonical_profile_name` only accepted
// `[a-z0-9-]` while the MCP server's `SAFE_CANONICAL_PROFILE_NAME` accepted underscores. The
// canonical key `team_coordinator` was therefore unloadable in Rust, and every Team spawn failed
// with `Missing canonical profile Some("team_coordinator")`. The tests below pin the shared
// charset, the coordinator load contract, and a guard covering every declared profile key.

#[test]
fn trusted_canonical_profile_name_matches_mcp_server_charset() {
    // Must stay identical to SAFE_CANONICAL_PROFILE_NAME in
    // plugins/app/ralphx-mcp-server/src/canonical-agent-metadata.ts:
    // ^[a-z0-9]+(?:[_-][a-z0-9]+)*$
    for accepted in ["plan", "team_coordinator", "a1", "a-b_c"] {
        assert_eq!(
            trusted_canonical_profile_name(accepted),
            Some(accepted),
            "canonical profile name {accepted:?} must be accepted"
        );
    }

    for rejected in [
        "",
        "Team_Coordinator",
        "_lead",
        "lead_",
        "team__lead",
        "team-_lead",
        "..",
        "../x",
        "a/b",
        "a\\b",
        "team coordinator",
        "team.coordinator",
    ] {
        assert_eq!(
            trusted_canonical_profile_name(rejected),
            None,
            "canonical profile name {rejected:?} must be rejected"
        );
    }
}

#[test]
fn team_coordinator_profile_resolves_for_rx_native_team_conversations() {
    let root = project_root();
    let definition = load_canonical_agent_definition_for_profile(
        &root,
        "ralphx-general-worker",
        Some("team_coordinator"),
    )
    .expect("missing team_coordinator profile for ralphx-general-worker");
    let runtime_config =
        get_agent_config_for_profile("ralphx-general-worker", Some("team_coordinator"))
            .expect("missing runtime team_coordinator profile for ralphx-general-worker");
    let preapproved_tools =
        get_preapproved_tools_for_profile("ralphx-general-worker", Some("team_coordinator"))
            .expect("missing preapproved tools for ralphx-general-worker team_coordinator profile");
    let claude_metadata = load_canonical_claude_metadata_for_profile(
        &root,
        "ralphx-general-worker",
        Some("team_coordinator"),
    );
    let codex_metadata = load_canonical_codex_metadata_for_profile(
        &root,
        "ralphx-general-worker",
        Some("team_coordinator"),
    );
    let runtime_profile_context = render_agent_runtime_profile_context(
        &root,
        "ralphx-general-worker",
        Some("team_coordinator"),
    )
    .expect("missing runtime profile context for team_coordinator");

    assert_eq!(definition.role, "team_coordinator");
    assert_eq!(
        definition.capabilities.mcp_tools, runtime_config.allowed_mcp_tools,
        "coordinator runtime MCP grants must come from canonical profile capabilities"
    );
    for team_tool in [
        "team_add_member",
        "team_assign",
        "team_list",
        "team_stop_member",
        "team_send_message",
    ] {
        assert!(
            runtime_config
                .allowed_mcp_tools
                .iter()
                .any(|tool| tool == team_tool),
            "coordinator must be granted {team_tool}"
        );
    }
    assert_eq!(
        definition.delegation.allowed_targets,
        vec![
            "ralphx-general-explorer".to_string(),
            "ralphx-general-worker".to_string()
        ]
    );

    let claude_tools = claude_metadata.tools.clone().expect("claude tools block");
    assert_eq!(claude_tools.extends, Some("readonly_tools".to_string()));
    for disallowed in ["Write", "Edit", "NotebookEdit", "Bash"] {
        assert!(
            claude_metadata
                .disallowed_tools
                .iter()
                .any(|tool| tool == disallowed),
            "coordinator must disallow {disallowed}"
        );
    }
    assert!(
        !preapproved_tools.contains("Bash") && !preapproved_tools.contains("Write"),
        "coordinator must not preapprove writable CLI tools, got: {preapproved_tools:?}"
    );
    assert_eq!(
        codex_metadata.runtime_features.get("shell_tool"),
        Some(&false),
        "coordinator must run without the Codex shell tool"
    );

    assert!(runtime_profile_context.contains("<profile_slug>team_coordinator</profile_slug>"));
    assert!(runtime_profile_context.contains("<profile_role>team_coordinator</profile_role>"));

    for harness in [AgentPromptHarness::Claude, AgentPromptHarness::Codex] {
        let prompt = load_harness_agent_prompt_for_profile(
            &root,
            "ralphx-general-worker",
            harness,
            Some("team_coordinator"),
        )
        .unwrap_or_else(|| panic!("missing {harness:?} coordinator prompt"));
        assert!(
            prompt.contains("team_add_member") && prompt.contains("team_assign"),
            "{harness:?} coordinator prompt must describe how writable work is delegated"
        );
        assert!(
            !prompt.contains("<rx_native_team_contract>"),
            "{harness:?} coordinator prompt must not duplicate the runtime-injected team contract"
        );
    }

    // The task-context coordinator shares the profile key but drops Task as well.
    let task_definition = load_canonical_agent_definition_for_profile(
        &root,
        "ralphx-chat-task",
        Some("team_coordinator"),
    )
    .expect("missing team_coordinator profile for ralphx-chat-task");
    let task_claude_metadata = load_canonical_claude_metadata_for_profile(
        &root,
        "ralphx-chat-task",
        Some("team_coordinator"),
    );
    assert_eq!(task_definition.role, "team_coordinator");
    for disallowed in ["Write", "Edit", "NotebookEdit", "Bash", "Task"] {
        assert!(
            task_claude_metadata
                .disallowed_tools
                .iter()
                .any(|tool| tool == disallowed),
            "task-context coordinator must disallow {disallowed}"
        );
    }
    assert!(
        load_harness_agent_prompt_for_profile(
            &root,
            "ralphx-chat-task",
            AgentPromptHarness::Claude,
            Some("team_coordinator"),
        )
        .is_some_and(|prompt| prompt.contains("team_add_member")),
        "task-context coordinator must ship a Claude profile prompt"
    );
}

/// RX-TEAM-007 / RX-TEAM-005a: `team_roster` routes to the read-only member roster handler,
/// which already accepts `MessageAuthority::Coordinator`; only the canonical grant was missing.
/// `team_status` is the new coordinator-only liveness projection. Both `team_coordinator`
/// profiles (general-worker, chat-task) must carry both grants end to end, and an agent outside
/// the Team surface must never receive either.
#[test]
fn team_roster_and_team_status_granted_for_team_coordinator_profile_on_both_agents() {
    let root = project_root();

    for agent_name in ["ralphx-general-worker", "ralphx-chat-task"] {
        let definition = load_canonical_agent_definition_for_profile(
            &root,
            agent_name,
            Some("team_coordinator"),
        )
        .unwrap_or_else(|| panic!("missing team_coordinator profile for {agent_name}"));
        let runtime_config = get_agent_config_for_profile(agent_name, Some("team_coordinator"))
            .unwrap_or_else(|| panic!("missing runtime team_coordinator profile for {agent_name}"));

        for team_tool in ["team_roster", "team_status"] {
            assert!(
                definition
                    .capabilities
                    .mcp_tools
                    .iter()
                    .any(|tool| tool == team_tool),
                "{agent_name} team_coordinator profile canonical capabilities should grant {team_tool}"
            );
            assert!(
                runtime_config
                    .allowed_mcp_tools
                    .iter()
                    .any(|tool| tool == team_tool),
                "{agent_name} team_coordinator profile runtime grants should include {team_tool}"
            );
        }
    }
}

/// Denied path: an agent outside the RX-native Team surface must not receive `team_roster` or
/// `team_status`.
#[test]
fn team_roster_and_team_status_denied_for_non_team_agent() {
    let root = project_root();
    let definition = load_canonical_agent_definition(&root, "ralphx-chat-project")
        .expect("expected canonical definition for ralphx-chat-project");
    let runtime_config = get_agent_config("ralphx-chat-project")
        .expect("expected runtime config for ralphx-chat-project");

    for team_tool in ["team_roster", "team_status"] {
        assert!(
            !definition
                .capabilities
                .mcp_tools
                .iter()
                .any(|tool| tool == team_tool),
            "ralphx-chat-project canonical capabilities must not grant {team_tool}"
        );
        assert!(
            !runtime_config
                .allowed_mcp_tools
                .iter()
                .any(|tool| tool == team_tool),
            "ralphx-chat-project runtime grants must not include {team_tool}"
        );
    }
}

/// Class-killer: any profile key declared anywhere in `agents/*/agent.yaml` must resolve through
/// the Rust catalog. A future key the validator rejects fails here instead of in a release build.
#[test]
fn every_declared_canonical_profile_resolves_through_the_catalog() {
    let root = project_root();
    let agent_names = list_canonical_agent_names(&root);
    assert!(
        agent_names.contains(&"ralphx-general-worker".to_string()),
        "canonical agent enumeration must see the live agents tree, got: {agent_names:?}"
    );

    let mut checked_profiles = Vec::new();
    for agent_name in &agent_names {
        let Some(definition) = load_canonical_agent_definition(&root, agent_name) else {
            continue;
        };
        for profile_name in definition.profiles.keys() {
            assert!(
                trusted_canonical_profile_name(profile_name).is_some(),
                "profile key {profile_name:?} on agent {agent_name} is rejected by \
                 trusted_canonical_profile_name; it can never load at spawn"
            );
            assert!(
                load_canonical_agent_definition_for_profile(&root, agent_name, Some(profile_name))
                    .is_some(),
                "profile {profile_name:?} on agent {agent_name} must resolve through the catalog"
            );
            assert!(
                try_load_canonical_claude_metadata_for_profile(
                    &root,
                    agent_name,
                    Some(profile_name)
                )
                .is_ok(),
                "profile {profile_name:?} on agent {agent_name} must load Claude metadata"
            );
            checked_profiles.push(format!("{agent_name}:{profile_name}"));
        }
    }

    assert!(
        checked_profiles.contains(&"ralphx-general-worker:team_coordinator".to_string())
            && checked_profiles.contains(&"ralphx-ideation:plan".to_string()),
        "guard must actually cover the live profile keys, got: {checked_profiles:?}"
    );
}

/// The coordinator profile exists to remove `Write`/`Edit`/`Bash`. An unresolvable profile must
/// error rather than silently hand back the base agent configuration.
#[test]
fn unresolvable_profiles_fail_closed_without_base_config_fallback() {
    let root = project_root();

    let missing = try_load_canonical_claude_metadata_for_profile(
        &root,
        "ralphx-general-worker",
        Some("does-not-exist"),
    )
    .expect_err("unknown profile must not fall back to the base agent metadata");
    assert!(
        missing.contains("Missing canonical profile"),
        "unknown profile should report a missing profile, got: {missing}"
    );

    let invalid = try_load_canonical_claude_metadata_for_profile(
        &root,
        "ralphx-general-worker",
        Some("Team_Coordinator"),
    )
    .expect_err("malformed profile name must not fall back to the base agent metadata");
    assert!(
        invalid.contains("Invalid canonical profile name"),
        "malformed profile name must be distinguishable from a missing profile, got: {invalid}"
    );

    assert!(
        get_agent_config_for_profile("ralphx-general-worker", Some("does-not-exist")).is_none(),
        "unknown profile must not resolve to the base runtime agent config"
    );
    assert!(
        load_canonical_agent_definition_for_profile(
            &root,
            "ralphx-general-worker",
            Some("does-not-exist")
        )
        .is_none(),
        "unknown profile must not overlay onto the base definition"
    );
}
