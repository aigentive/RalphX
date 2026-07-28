//! Exhaustive capability policy for the live Tauri command census.
//!
//! The ledger is deliberately module-defaulted with narrow command overrides. Command modules
//! are RalphX's established risk boundary, so this records the actual audit method instead of
//! pretending 500+ independent judgements were made. Tests expand these policies against
//! `commands/registry.rs`; an unknown module or duplicate command fails the census gate.

use ralphx_remote_protocol::{Capability, RiskClass};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerPolicy {
    pub class: RiskClass,
    pub capabilities: &'static [Capability],
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleDefault {
    pub module: &'static str,
    pub policy: LedgerPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandOverride {
    pub command: &'static str,
    pub policy: LedgerPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityReducingExemption {
    pub subject: &'static str,
    pub kind: &'static str,
    pub direction: &'static str,
    pub scope: &'static str,
    pub rationale: &'static str,
}

const NONE: &[Capability] = &[];
const AGENT: &[Capability] = &[Capability::AgentControl];
const SEEDS_STATE: &[Capability] = &[Capability::SeedsSpawnTriggeringState];
const MUTATES_CONTENT: &[Capability] = &[Capability::MutatesAgentConsumedContent];
const AGENT_AND_CONTENT: &[Capability] = &[
    Capability::AgentControl,
    Capability::MutatesAgentConsumedContent,
];
const PROCESS: &[Capability] = &[Capability::SpawnsProcess];
const CREDENTIALS: &[Capability] = &[Capability::TouchesCredentials];
const PTY: &[Capability] = &[Capability::PtyControl];
const HOST: &[Capability] = &[Capability::HostManagement];
const PATH: &[Capability] = &[Capability::WritesArbitraryPath];
const FUTURE_PROCESS: &[Capability] = &[Capability::ConfiguresFutureProcessAuthority];
const DELETE: &[Capability] = &[Capability::DeletesEntity];

const fn policy(
    class: RiskClass,
    capabilities: &'static [Capability],
    reason: &'static str,
) -> LedgerPolicy {
    LedgerPolicy {
        class,
        capabilities,
        reason,
    }
}

const fn agent_default(module: &'static str) -> ModuleDefault {
    ModuleDefault {
        module,
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "conservative-module-default: may steer or arm autonomous work",
        ),
    }
}

const fn elevated_default(
    module: &'static str,
    capabilities: &'static [Capability],
    reason: &'static str,
) -> ModuleDefault {
    ModuleDefault {
        module,
        policy: policy(RiskClass::Elevated, capabilities, reason),
    }
}

/// Every module in the live registry must have exactly one default here.
pub const MODULE_DEFAULTS: &[ModuleDefault] = &[
    agent_default("root"),
    agent_default("activity_commands"),
    agent_default("agent_composer_commands"),
    elevated_default(
        "agent_issue_report_commands",
        PROCESS,
        "report construction may spawn diagnostics",
    ),
    agent_default("agent_model_commands"),
    agent_default("agent_plan_commands"),
    agent_default("agent_profile_commands"),
    agent_default("agent_sidebar_commands"),
    elevated_default("agent_terminal_commands", PTY, "terminal PTY control"),
    elevated_default("api_key_commands", CREDENTIALS, "credential surface"),
    agent_default("artifact_commands"),
    elevated_default(
        "atlassian_commands",
        CREDENTIALS,
        "integration credential surface",
    ),
    agent_default("automation_commands"),
    elevated_default(
        "chat_attachment_commands",
        PATH,
        "attachment filesystem surface",
    ),
    elevated_default(
        "clickup_commands",
        CREDENTIALS,
        "integration credential surface",
    ),
    agent_default("conversation_folder_reference_commands"),
    agent_default("conversation_stats_commands"),
    elevated_default(
        "diagnostic_commands",
        PROCESS,
        "diagnostics may spawn provider CLIs",
    ),
    elevated_default("diff_commands", PROCESS, "diff getters may spawn git"),
    agent_default("execution_commands"),
    elevated_default(
        "external_mcp_commands",
        CREDENTIALS,
        "external MCP credential surface",
    ),
    elevated_default(
        "git_commands",
        PROCESS,
        "git process and worktree authority",
    ),
    elevated_default(
        "github_commands",
        PROCESS,
        "GitHub CLI/network process authority",
    ),
    elevated_default(
        "granola_commands",
        CREDENTIALS,
        "integration credential surface",
    ),
    elevated_default(
        "harness_provider_commands",
        FUTURE_PROCESS,
        "configures future provider process authority",
    ),
    agent_default("health"),
    agent_default("ideation_commands"),
    elevated_default(
        "linear_commands",
        CREDENTIALS,
        "integration credential surface",
    ),
    agent_default("manual_role_default_commands"),
    agent_default("mcp_policy_commands"),
    agent_default("merge_pipeline_commands"),
    agent_default("methodology_commands"),
    agent_default("metrics_commands"),
    agent_default("notification_commands"),
    agent_default("permission_commands"),
    agent_default("persona_commands"),
    elevated_default(
        "plan_branch_commands",
        PROCESS,
        "branch operations spawn git",
    ),
    agent_default("plan_commands"),
    elevated_default(
        "project_commands",
        PROCESS,
        "project git/gh and deferred shell authority",
    ),
    elevated_default(
        "provider_cli_management_commands",
        PROCESS,
        "provider CLI installer surface",
    ),
    agent_default("qa_commands"),
    agent_default("question_commands"),
    agent_default("release_notes_commands"),
    elevated_default(
        "remote_device_commands",
        HOST,
        "remote host/device authority",
    ),
    elevated_default(
        "remote_environment_commands",
        HOST,
        "remote environment authority",
    ),
    elevated_default("remote_host_commands", HOST, "remote listener authority"),
    elevated_default(
        "remote_transport_spike_commands",
        HOST,
        "debug remote transport authority",
    ),
    elevated_default(
        "repository_settings_commands",
        FUTURE_PROCESS,
        "configures repository process authority",
    ),
    agent_default("research_commands"),
    agent_default("review_commands"),
    elevated_default(
        "startup_commands",
        HOST,
        "startup and log-management authority",
    ),
    agent_default("task_commands"),
    agent_default("task_context_commands"),
    agent_default("task_step_commands"),
    elevated_default(
        "test_data_commands",
        DELETE,
        "test-data mutation is never remotely operable",
    ),
    elevated_default(
        "ticketing_commands",
        CREDENTIALS,
        "ticket integration credential/network surface",
    ),
    agent_default("ui_commands"),
    agent_default("unified_chat_commands"),
    agent_default("update_channel_commands"),
    agent_default("validation_commands"),
    agent_default("workflow_commands"),
    elevated_default(
        "workspace_open_commands",
        PROCESS,
        "opens workspace in an external process",
    ),
    agent_default("workspace_review_settings_commands"),
];

/// Narrow decisions which differ from their module default.
pub const COMMAND_OVERRIDES: &[CommandOverride] = &[
    CommandOverride {
        command: "inject_task",
        policy: policy(
            RiskClass::AgentControl,
            SEEDS_STATE,
            "detector-b: seeds internal_status=Ready consumed by the ready-task scheduler",
        ),
    },
    CommandOverride {
        command: "resume_automation",
        policy: policy(
            RiskClass::AgentControl,
            SEEDS_STATE,
            "detector-b: restores Active automation consumed by the automation scheduler",
        ),
    },
    CommandOverride {
        command: "finalize_automation",
        policy: policy(
            RiskClass::AgentControl,
            SEEDS_STATE,
            "detector-b: completes automation arming state consumed by the automation scheduler",
        ),
    },
    CommandOverride {
        command: "create_task_step",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: creates worker-consumed task step",
        ),
    },
    CommandOverride {
        command: "update_task_step",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: updates worker-consumed task step",
        ),
    },
    CommandOverride {
        command: "create_artifact",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: creates worker-consumed artifact of any kind",
        ),
    },
    CommandOverride {
        command: "update_artifact",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: updates worker-consumed artifact of any kind",
        ),
    },
    CommandOverride {
        command: "add_artifact_relation",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: changes worker-consumed artifact relations",
        ),
    },
    CommandOverride {
        command: "update_task_proposal",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: updates worker-consumed task proposal",
        ),
    },
    CommandOverride {
        command: "approve_review",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: writes worker-consumed review feedback",
        ),
    },
    CommandOverride {
        command: "reject_review",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: writes worker-consumed review feedback",
        ),
    },
    CommandOverride {
        command: "request_changes",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: writes worker-consumed review feedback",
        ),
    },
    CommandOverride {
        command: "reject_fix_task",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: writes worker-consumed fix feedback",
        ),
    },
    CommandOverride {
        command: "approve_task_for_review",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: writes worker-consumed review note",
        ),
    },
    CommandOverride {
        command: "request_task_changes_for_review",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: writes worker-consumed review feedback",
        ),
    },
    CommandOverride {
        command: "request_task_changes_from_reviewing",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: writes worker-consumed review feedback",
        ),
    },
    CommandOverride {
        command: "move_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-a plus content-surface: restart note is worker-consumed",
        ),
    },
    // Audited read-only registrations plus the two Wry-monomorphic reads which cannot yet be
    // registered through `remote_commands!` (facade runtime genericity; deferred to PR 3.1).
    CommandOverride {
        command: "health_check",
        policy: policy(RiskClass::Read, NONE, "pure health read"),
    },
    CommandOverride {
        command: "list_tasks",
        policy: policy(RiskClass::Read, NONE, "task read"),
    },
    CommandOverride {
        command: "get_task",
        policy: policy(RiskClass::Read, NONE, "task read"),
    },
    CommandOverride {
        command: "search_tasks",
        policy: policy(RiskClass::Read, NONE, "task read"),
    },
    CommandOverride {
        command: "get_valid_transitions",
        policy: policy(RiskClass::Read, NONE, "state-machine metadata read"),
    },
    CommandOverride {
        command: "list_remote_advertised_endpoints",
        policy: policy(
            RiskClass::Read,
            NONE,
            "remote endpoint read; AppHandle-ineligible until PR 3.1",
        ),
    },
    CommandOverride {
        command: "list_remote_audit_entries",
        policy: policy(
            RiskClass::Read,
            NONE,
            "remote audit read; AppHandle-ineligible until PR 3.1",
        ),
    },
    // Target-sensitive authority-reducing exemptions.
    CommandOverride {
        command: "pause_task",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: transitions only to Paused",
        ),
    },
    CommandOverride {
        command: "block_task",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: transitions only to Blocked",
        ),
    },
    CommandOverride {
        command: "stop_task",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: transitions only to Stopped",
        ),
    },
    CommandOverride {
        command: "pause_tasks_in_group",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: transitions only to Paused",
        ),
    },
    CommandOverride {
        command: "deny_permission_request",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: denies a live tool call",
        ),
    },
    CommandOverride {
        command: "unblock_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "authority-restoring transition",
        ),
    },
    CommandOverride {
        command: "reanalyze_project",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "spawns the project-analyzer agent",
        ),
    },
    // Declared memberships not inferable from transition/process sinks.
    CommandOverride {
        command: "resolve_permission_request",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "approve branch authorizes-live-tool-call; deny branch is authority-reducing",
        ),
    },
    CommandOverride {
        command: "resolve_user_question",
        policy: policy(RiskClass::AgentControl, AGENT, "steering-question"),
    },
];

pub const AUTHORITY_REDUCING_EXEMPTIONS: &[AuthorityReducingExemption] = &[
    AuthorityReducingExemption {
        subject: "pause_task",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "transitions only to Paused",
    },
    AuthorityReducingExemption {
        subject: "block_task",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "transitions only to Blocked",
    },
    AuthorityReducingExemption {
        subject: "stop_task",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "transitions only to Stopped",
    },
    AuthorityReducingExemption {
        subject: "pause_tasks_in_group",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "transitions only to Paused",
    },
    AuthorityReducingExemption {
        subject: "deny_permission_request",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "denies a live tool call",
    },
    AuthorityReducingExemption {
        subject: "Cancelled",
        kind: "transition-target",
        direction: "authority-reducing",
        scope: "transition-target",
        rationale: "domain/state_machine/transition_handler/mod.rs on_exit stops pollers for Cancelled; on_enter_states/mod.rs has no Cancelled entry action",
    },
    AuthorityReducingExemption {
        subject: "Archived",
        kind: "transition-target",
        direction: "authority-reducing",
        scope: "transition-target",
        rationale: "domain/state_machine/transition_handler/on_enter_states/mod.rs has no Archived entry action and application reconciliation does not scan Archived tasks",
    },
];

pub const DECLARED_MEMBERSHIPS: &[(&str, &str)] = &[
    ("approve_permission_request", "authorizes-live-tool-call"),
    ("resolve_user_question", "steering-question"),
];

/// Expands the module policy and command overrides into one effective command row.
pub fn policy_for(command: &str, module: &str) -> Option<LedgerPolicy> {
    if let Some(entry) = COMMAND_OVERRIDES
        .iter()
        .find(|entry| entry.command == command)
    {
        return Some(entry.policy);
    }
    if command.starts_with("delete_") {
        return Some(policy(
            RiskClass::Elevated,
            DELETE,
            "deletes a durable entity",
        ));
    }
    MODULE_DEFAULTS
        .iter()
        .find(|entry| entry.module == module)
        .map(|entry| entry.policy)
}
