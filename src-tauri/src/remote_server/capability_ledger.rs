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
const AGENT_AND_SEEDS: &[Capability] = &[
    Capability::AgentControl,
    Capability::SeedsSpawnTriggeringState,
];
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

const fn denied_default(
    module: &'static str,
    capabilities: &'static [Capability],
    reason: &'static str,
) -> ModuleDefault {
    ModuleDefault {
        module,
        policy: policy(RiskClass::Denied, capabilities, reason),
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
    denied_default("agent_terminal_commands", PTY, "terminal PTY control"),
    denied_default("api_key_commands", CREDENTIALS, "credential surface"),
    agent_default("artifact_commands"),
    denied_default(
        "atlassian_commands",
        CREDENTIALS,
        "integration credential surface",
    ),
    agent_default("automation_commands"),
    denied_default(
        "chat_attachment_commands",
        PATH,
        "attachment filesystem surface",
    ),
    denied_default(
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
    denied_default(
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
    denied_default(
        "granola_commands",
        CREDENTIALS,
        "integration credential surface",
    ),
    denied_default(
        "harness_provider_commands",
        FUTURE_PROCESS,
        "configures future provider process authority",
    ),
    agent_default("health"),
    agent_default("ideation_commands"),
    denied_default(
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
    denied_default(
        "provider_cli_management_commands",
        PROCESS,
        "provider CLI installer surface",
    ),
    agent_default("qa_commands"),
    agent_default("question_commands"),
    agent_default("release_notes_commands"),
    // The module is spawn-free by construction (no AppHandle, no ExecutionState, no
    // ChatService), but the default stays conservative: a future member must earn a
    // narrower row rather than inherit one.
    agent_default("remote_chat_commands"),
    // Same construction and the same conservative default as `remote_chat_commands`: the
    // module cannot spawn (no AppHandle, no ExecutionState, no ChatService), but a future
    // member must still earn its own row rather than inherit a narrow one.
    agent_default("remote_transcript_commands"),
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
    denied_default(
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
    denied_default(
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
    // R5-H1 names these two detector-(b) writers explicitly. Without the tag the manifest
    // records the class but not WHY the class is required, and the evidence could be dropped
    // from either row without CI noticing.
    CommandOverride {
        command: "set_agent_conversation_workspace_auto_publish",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_SEEDS,
            "detector-b: arms auto_publish_enabled consumed by the auto-publish freshness scan",
        ),
    },
    CommandOverride {
        command: "update_review_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_SEEDS,
            "detector-b: arms require_workspace_review consumed by the auto-review spawner",
        ),
    },
    // Detector-(d) writers: each names a content repository handle and a write verb in its own
    // body, so the §3.3 backstop-#2 gate requires the capability on the row.
    CommandOverride {
        command: "start_step",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed task step status",
        ),
    },
    CommandOverride {
        command: "complete_step",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed task step status",
        ),
    },
    CommandOverride {
        command: "skip_step",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed task step status",
        ),
    },
    CommandOverride {
        command: "fail_step",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed task step status",
        ),
    },
    CommandOverride {
        command: "verify_issue",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed review issue state",
        ),
    },
    CommandOverride {
        command: "reopen_issue",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed review issue state",
        ),
    },
    CommandOverride {
        command: "mark_issue_in_progress",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed review issue state",
        ),
    },
    CommandOverride {
        command: "mark_issue_addressed",
        policy: policy(
            RiskClass::AgentControl,
            AGENT_AND_CONTENT,
            "detector-d: writes worker-consumed review issue state",
        ),
    },
    // Reviewed rather than inherited. The module default would already say AgentControl,
    // but it would say it for the wrong reason ("may steer or arm autonomous work"), and
    // this command does neither: it writes transcript content a live agent will read.
    CommandOverride {
        command: "send_remote_chat_message",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: queues a role-pinned user turn for a run that is already live; \
             detector-silent on (a), (b) and (c) — it arms no scheduler, resolves no CLI path, \
             and refuses when no live run would drain the row, so a message can never be \
             persisted as sent yet delivered to nobody. The role is pinned to \"user\" at \
             dispatch, so a remote client cannot forge an orchestrator speaker label",
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
    CommandOverride {
        command: "switch_git_origin_to_ssh",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "changes repository origin authentication",
        ),
    },
    CommandOverride {
        command: "setup_gh_git_auth",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "configures git credential authority",
        ),
    },
    CommandOverride {
        command: "login_gh_with_browser",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "starts interactive GitHub authentication",
        ),
    },
    CommandOverride {
        command: "update_custom_analysis",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "executes the canonical deferred shell-authority shape",
        ),
    },
    CommandOverride {
        command: "change_project_git_mode",
        policy: policy(RiskClass::Denied, PROCESS, "changes project git authority"),
    },
    CommandOverride {
        command: "get_git_branches",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "spawns git over project-controlled state",
        ),
    },
    CommandOverride {
        command: "resolve_merge_conflict",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "destructive merge-conflict resolution",
        ),
    },
    CommandOverride {
        command: "cleanup_task_branch",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "destructive task branch cleanup",
        ),
    },
    CommandOverride {
        command: "cleanup_task",
        policy: policy(RiskClass::Denied, AGENT, "destructive task cleanup"),
    },
    CommandOverride {
        command: "cleanup_tasks_in_group",
        policy: policy(
            RiskClass::Denied,
            AGENT,
            "destructive task cleanup across a whole group",
        ),
    },
    CommandOverride {
        command: "publish_agent_conversation_workspace",
        policy: policy(
            RiskClass::Denied,
            AGENT,
            "publishes an agent conversation workspace",
        ),
    },
    CommandOverride {
        command: "close_agent_workspace_pr",
        policy: policy(
            RiskClass::Denied,
            AGENT,
            "closes the remote pull request an agent workspace published",
        ),
    },
    CommandOverride {
        command: "update_agent_conversation_workspace_from_base",
        policy: policy(
            RiskClass::Denied,
            AGENT,
            "rewrites an agent workspace checkout from its base branch",
        ),
    },
    CommandOverride {
        command: "get_task_file_changes",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "spawns git for task file changes",
        ),
    },
    CommandOverride {
        command: "get_file_diff",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "spawns git for an arbitrary file diff",
        ),
    },
    CommandOverride {
        command: "get_codex_cli_diagnostics",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "spawns the Codex CLI for diagnostics",
        ),
    },
    CommandOverride {
        command: "build_agent_issue_report",
        policy: policy(
            RiskClass::Denied,
            PROCESS,
            "spawns diagnostic report tooling",
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
    // PR 3.1-b batch 1 — the 2.7 reconnect gate reads. Hand audit (detectors a/b/c all
    // silent, asserted by the calibration lists in `capability_ledger_tests`):
    //
    // * `list_pending_permission_gates` → `PermissionState::get_pending_info_strict`, which
    //   reads the pending repository and the in-memory pending map and returns their union.
    //   It resolves no CLI path (c), writes no `InternalStatus` (a), and touches none of the
    //   spawn-triggering state surface (b). The `_strict` suffix is the fail-closed half of
    //   the pair: it propagates a repository error instead of collapsing to an empty list,
    //   so registering it cannot turn a read failure into "no gates are open".
    // * `list_pending_question_gates` → `QuestionState::get_pending_info_strict`, the same
    //   shape over the question gate surface.
    //
    // Both sit BELOW their `permission_commands`/`question_commands` module default
    // (AgentControl), so each needs a structural reason rather than a judgement call: the
    // module default is conservative because those modules also carry
    // `resolve_permission_request`/`resolve_user_question`, which authorize or steer a LIVE
    // tool call. These two only enumerate what is already pending — they answer no gate and
    // change no agent's authority.
    CommandOverride {
        command: "list_pending_permission_gates",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pending permission-gate enumeration: fail-closed read of the pending repository \
             plus the in-memory gate map; resolves no gate and arms no scheduling",
        ),
    },
    CommandOverride {
        command: "list_pending_question_gates",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pending question-gate enumeration: fail-closed read of the pending repository \
             plus the in-memory gate map; answers no question and arms no scheduling",
        ),
    },
    // PR 3.1-b batch 2 — census `B1`, `task_commands` read cluster.
    //
    // Every row here previously resolved through the `task_commands` `agent_default`, i.e. it
    // carried "conservative-module-default: may steer or arm autonomous work" — a placeholder,
    // not a reviewed judgement. Each was run through the live `authority_audit` call graph
    // (detectors (a), (b) and (c) all silent for all seven; the claim is pinned by the
    // calibration lists in `capability_ledger_tests`) and then hand-traced to its repository
    // call. The module default stays `AgentControl` because the module also holds
    // `move_task`/`unblock_task`/`inject_task`/`resume_execution_plan`; these seven only read.
    //
    // The shared structural reason: each body is a repository query whose error is propagated
    // (`map_err(...)?` / `?`), never collapsed into an empty or default result. A read failure
    // therefore cannot be presented to a remote client as "nothing here" — the fail-open shape
    // is what disqualifies `get_pending_permissions`/`get_pending_questions` below.
    CommandOverride {
        command: "get_archived_count",
        policy: policy(
            RiskClass::Read,
            NONE,
            "archived-task count: `task_repo.get_archived_count`, a scalar count read",
        ),
    },
    CommandOverride {
        command: "get_tasks_awaiting_review",
        policy: policy(
            RiskClass::Read,
            NONE,
            "review-queue read: `task_repo.list_paginated` filtered to the four review \
             statuses; selects rows and starts no review",
        ),
    },
    CommandOverride {
        command: "get_session_task_history_availability",
        policy: policy(
            RiskClass::Read,
            NONE,
            "session history availability: `task_repo.count_tasks` rendered as a bool plus \
             a count",
        ),
    },
    CommandOverride {
        command: "get_task_state_transitions",
        policy: policy(
            RiskClass::Read,
            NONE,
            "status-history read: `task_repo.get_status_history` mapped to a response; \
             reads transitions already taken and requests none",
        ),
    },
    CommandOverride {
        command: "get_task_dependency_graph",
        policy: policy(
            RiskClass::Read,
            NONE,
            "dependency-graph read: in-process traversal over `task_repo` rows; writes no \
             edge and schedules nothing",
        ),
    },
    CommandOverride {
        command: "get_task_timeline_events",
        policy: policy(
            RiskClass::Read,
            NONE,
            "timeline read: derives events from `task_repo` rows in process",
        ),
    },
    CommandOverride {
        command: "get_task_agent_workspace",
        policy: policy(
            RiskClass::Read,
            NONE,
            "workspace-association read: joins the task's plan-branch and agent-conversation \
             workspace rows; resolves no CLI and touches no filesystem",
        ),
    },
    // PR 3.1-b batch 2 — census `B1`, `task_step_commands` read cluster.
    //
    // The module default stays `AgentControl` and the rest of the module keeps it: the step
    // WRITES (`create_task_step`, `update_task_step`, `reorder_task_steps`, `start_step`,
    // `complete_step`, `skip_step`, `fail_step`) are worker-consumed content, which is why
    // two of them already carry explicit `MutatesAgentConsumedContent` overrides. Reading
    // steps consumes that content; it does not author it.
    CommandOverride {
        command: "get_task_steps",
        policy: policy(
            RiskClass::Read,
            NONE,
            "task-step read: `task_step_repo.get_by_task` mapped to responses",
        ),
    },
    CommandOverride {
        command: "get_step_progress",
        policy: policy(
            RiskClass::Read,
            NONE,
            "step-progress read: `task_step_repo.get_by_task` summarised in process",
        ),
    },
    // PR 3.1-b batch 2 — census `B1`, `execution_commands` read cluster.
    //
    // Deliberately narrow. The module default stays `AgentControl`, and three sibling
    // getters are NOT reclassified for two distinct, individually-audited reasons:
    //
    // * `get_execution_status` and `get_running_processes` — detector (c) FIRES on both.
    //   They resolve a process-inspection CLI, and `SpawnsProcess` is expressible only
    //   under `Elevated`, so ledgering either `Read` would be the exact `list_projects`
    //   under-labelling shape. They stay above `Read` and unregistered.
    // * `set_active_project` — detectors are silent, but the hand-trace disqualifies it:
    //   after persisting, it calls `sync_quota_from_project`, which writes the runtime
    //   `ExecutionState` concurrency quota. Raising a quota is how waiting `Ready` tasks
    //   get picked up, so it is scheduler-arming authority that no detector models.
    CommandOverride {
        command: "get_execution_settings",
        policy: policy(
            RiskClass::Read,
            NONE,
            "execution-settings read: `execution_settings_repo.get_settings`; reads the \
             quota and changes none",
        ),
    },
    CommandOverride {
        command: "get_global_execution_settings",
        policy: policy(
            RiskClass::Read,
            NONE,
            "global execution-settings read: `global_execution_settings_repo.get_settings`",
        ),
    },
    CommandOverride {
        command: "get_active_project",
        policy: policy(
            RiskClass::Read,
            NONE,
            "active-project read: clones the in-memory `ActiveProjectState` id; the WRITE \
             half (`set_active_project`) syncs the scheduler quota and stays AgentControl",
        ),
    },
    // Detector (c) finding: the advertised-endpoint listing resolves the Tailscale CLI, so it
    // spawns a process. `SpawnsProcess` is expressible only under `Elevated`; the previous
    // `Read` row was the same under-labelling shape as the `list_projects` mislabel.
    CommandOverride {
        command: "list_remote_advertised_endpoints",
        policy: policy(
            RiskClass::Elevated,
            PROCESS,
            "resolves the Tailscale CLI to enumerate advertised endpoints",
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
    // PR 1.5 `ui:operate` mutating surface. Both sit BELOW their `task_commands` module default
    // (AgentControl) and each needs a structural reason, not a judgement call.
    CommandOverride {
        command: "update_task",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "inert fields only at this class: category is a closed enum and priority an i32; \
             title/description carry a conditional MutatesAgentConsumedContent discharged by \
             update_task_authz, and internal_status is rejected by validate_update_task_input",
        ),
    },
    CommandOverride {
        command: "create_task",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "Backlog-only by construction: CreateTaskInput carries no status field and \
             Task::new_with_category sets InternalStatus::Backlog, so a created task cannot be \
             born in a spawn-triggering state",
        ),
    },
    // -------------------------------------------------------------------------------------
    // PR 3.1-b batch 3 — census `B2`, the conversation-stats read cluster.
    //
    // `B2` is the census's highest-risk batch and this is deliberately the smallest complete
    // module in it. All four audit detector-silent on (a)/(b)/(c)/(d), and each body was
    // hand-traced to repository reads whose errors are propagated with `map_err(...)?` rather
    // than collapsed into an empty result — the `get_pending_permissions` fail-open shape that
    // kept two batch-1 candidates unregistered does not appear here.
    //
    // The payloads are token/cost AGGREGATES — usage totals, coverage counts and per-harness,
    // per-model and per-effort buckets. No message text, no prompt, no tool input. This is a
    // usage-reporting surface, not the transcript surface; the transcript reads stay at the
    // module default and are the next batch's problem.
    // -----------------------------------------------------------------------------------
    // Batch 4 — the B2 detector-silent getters.
    //
    // Method, unchanged from batches 2 and 3: start from live detector output
    // (`probe_b2_module_batch_audit`, all 56 members), then hand-trace every candidate body,
    // because detector silence is necessary and never sufficient. Batches 2 and 3 each found
    // one detector-silent command that had to be refused; this pass found SEVEN, which is why
    // only five of seventeen candidates are registered here.
    //
    // Every row below satisfies the same three properties, each asserted:
    //   1. propagates read errors — no `unwrap_or_default`/`.ok()`/`Ok(vec![])` on an Err
    //      branch. A remote client is never told "no data" when the truth is "the query
    //      failed"; that fail-open shape is what disqualified two batch-1 candidates and four
    //      of this batch's.
    //   2. no write of any kind, including in-memory registry cleanup.
    //   3. takes `&AppState` only — no `tauri::AppHandle`, the spawn-authority carrier.
    CommandOverride {
        command: "get_agent_conversation_summary",
        policy: policy(
            RiskClass::Read,
            NONE,
            "conversation metadata without messages; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_agent_conversation_runtime_index",
        policy: policy(
            RiskClass::Read,
            NONE,
            "runtime lifecycle index via the non-mutating direct_agent_running_state_for_context path; propagates read errors",
        ),
    },
    CommandOverride {
        command: "list_agent_conversation_workspace_publication_events",
        policy: policy(
            RiskClass::Read,
            NONE,
            "workspace publication event history; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_bulk_workspace_publication_states",
        policy: policy(
            RiskClass::Read,
            NONE,
            "publication state enum and label per conversation; propagates read errors",
        ),
    },
    CommandOverride {
        command: "list_agent_models",
        policy: policy(
            RiskClass::Read,
            NONE,
            "built-in and custom model registry merge; propagates read errors",
        ),
    },
    // -----------------------------------------------------------------------------------
    // Batch 4 — the spawn-free transcript reads (the PR 3.2 dependency).
    //
    // The LOCAL `get_agent_conversation` / `..._messages_page` / `..._timeline_page` all fire
    // detector (a) and stay unregistered. `probe_transcript_read_arming_paths` shows why: each
    // opens with `wake_agent_workspace_for_bridge_events*`, which reaches the `send_message`
    // STEER sink. The wake is incidental to the read — the local commands themselves discard
    // its error with `tracing::warn!` and read anyway — so the answer is a seam split, not a
    // reclassification of the local command.
    //
    // These three are the pure-read variants. Each delegates to an existing `*_for_app_state`
    // seam, forks no logic, and takes only `&AppState`, so the wake is unreachable by
    // construction rather than by review. `remote_transcript_reads_never_reach_the_wake`
    // asserts that mechanically against the same call graph the detector uses.
    //
    // Content note, made explicitly because it is the reason these are the batch's most
    // scrutinised rows: unlike the B2 stats cluster, these payloads DO carry message text and
    // tool call/result blocks. That is the whole point — a remote transcript view is what PR
    // 3.2 exists to validate — and it is why they sit at `ui:read` and carry no capability,
    // rather than being folded into a lower-visibility batch. The page reads apply the same
    // `preview_tool_payloads_for_message` truncation the local UI gets; the un-truncated
    // escape hatches (`get_agent_message_tool_call_detail` and its timeline twin) are
    // deliberately NOT registered.
    CommandOverride {
        command: "get_remote_agent_conversation",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of a conversation and its messages; no wake, no spawn; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_messages_page",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of a message page; no wake, no spawn; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_timeline_page",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of a timeline page; no wake, no spawn; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_agent_conversation_stats",
        policy: policy(
            RiskClass::Read,
            NONE,
            "aggregates conversation/message/run repository reads into usage totals; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_project_chat_usage_stats",
        policy: policy(
            RiskClass::Read,
            NONE,
            "project-scoped usage aggregation over repository reads; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_task_chat_usage_stats",
        policy: policy(
            RiskClass::Read,
            NONE,
            "task-scoped usage aggregation over repository reads; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_insights_chat_usage_stats",
        policy: policy(
            RiskClass::Read,
            NONE,
            "project-or-all-projects usage aggregation over repository reads; propagates read errors",
        ),
    },

    // -------------------------------------------------------------------------------------
    // PR 3.1-b batch 3 — the Operate brakes.
    //
    // Batch 2 registered the `B1` reads and left the module defaults at `AgentControl`. These
    // three are the halting half of that remainder: each moves the system strictly toward
    // less autonomous work and none can start, resume, or steer any of it. They close a real
    // asymmetry — before this batch a remote viewer could watch execution it had no way to
    // stop.
    //
    // Detectors (a)/(b)/(c)/(d) are silent on all three (`probe_operate_brakes_audit`), but
    // detector silence is necessary and never sufficient here: `pause_execution` and
    // `stop_execution` both call `sync_quota_from_project`, which is the exact write that
    // disqualified `set_active_project` in batch 2. The distinction is proven, not asserted —
    // see `the_brake_quota_write_is_dominated_by_the_pause_flag`.
    CommandOverride {
        command: "pause_execution",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: gates scheduling and transitions agent-active tasks only to Paused",
        ),
    },
    CommandOverride {
        command: "stop_execution",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: gates scheduling and transitions agent-active tasks only to Stopped",
        ),
    },
    CommandOverride {
        command: "cancel_tasks_in_group",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing: transitions only to Cancelled",
        ),
    },
    // NOT reclassified — `archive_tasks_in_group` stays at the `task_commands` AgentControl
    // default. It is the batch-3 counterpart of batch 2's `set_active_project`: detector-silent,
    // superficially a sibling of the bulk brakes, and disqualified only by hand-tracing.
    // Archiving writes `archived_at` and nothing else — there is no `InternalStatus::Archived`,
    // so the ledger's `Archived` transition-target exemption does not reach this command. An
    // archived Executing task keeps its agent process, keeps its execution slot, and becomes
    // invisible to the reconciler (`get_by_status` filters `archived_at IS NULL`) while
    // `transition_task` refuses every recovery. That is authority-OBSCURING, not
    // authority-reducing. Pinned by `bulk_archive_is_not_a_brake_and_stays_unregistered`.

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
    // The approve half of the pinned permission split. Its sibling `deny_permission_request`
    // carries an authority-reducing exemption down to Operate; this half gets none, because
    // authorizing a live tool call is the declared membership itself.
    CommandOverride {
        command: "approve_permission_request",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "declared membership: authorizes-live-tool-call (server-pinned allow decision)",
        ),
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
    // PR 3.1-b batch 3 — the Operate brakes.
    AuthorityReducingExemption {
        subject: "pause_execution",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "commands/execution_commands/lifecycle.rs sets the pause flag and transitions agent-active tasks only to Paused; commands/execution_commands/state.rs can_start_task returns false on is_paused before reading any quota",
    },
    AuthorityReducingExemption {
        subject: "stop_execution",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "commands/execution_commands/lifecycle.rs sets the pause flag and transitions agent-active tasks only to Stopped; the only production caller of ExecutionState::resume is resume_execution, which re-syncs the quota first",
    },
    AuthorityReducingExemption {
        subject: "cancel_tasks_in_group",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "commands/task_commands/mutation.rs skips terminal tasks and transitions only to Cancelled, whose on_exit in domain/state_machine/transition_handler/mod.rs stops pollers and decrements the running count",
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

/// A capability a command carries only for SOME arguments.
///
/// `class_permits(Operate, [MutatesAgentConsumedContent])` is a compile error — `Operate` permits
/// no capability at all — so §3.3's "conditional capability" cannot be a macro `caps:` entry. It
/// is recorded here instead, and `conditional_capabilities_are_discharged_by_a_live_predicate`
/// makes the annotation and the argument-sensitive predicate inseparable: dropping the predicate
/// while the annotation stands (or the reverse) fails CI. Without that tie, `update_task` would
/// silently become a `ui:operate` write of worker-consumed prompt text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionalCapability {
    pub command: &'static str,
    pub capability: Capability,
    /// The argument condition under which the capability applies, and what discharges it.
    pub condition: &'static str,
}

pub const CONDITIONAL_CAPABILITIES: &[ConditionalCapability] = &[ConditionalCapability {
    command: "update_task",
    capability: Capability::MutatesAgentConsumedContent,
    condition: "conditional: title,description — discharged by update_task_authz",
}];

/// A registered command whose remote form is confined to an explicit scope argument.
///
/// Some commands are safe remotely only in a narrowed form. The brakes are the canonical case:
/// `pause_execution`/`stop_execution` take `project_id: Option<String>`, and the `None` arm
/// falls back to the LOCAL user's active project or, failing that, to `project_repo.get_all()`
/// — so a default-paired phone could sweep every project on the host in one call. A remote
/// device must name the project it is halting.
///
/// Recorded here rather than left implicit in the macro so the annotation and the `validate:`
/// predicate are inseparable: `scope_confinements_are_enforced_by_a_live_predicate` asserts the
/// tie in BOTH directions, so removing the predicate while the annotation stands (or the
/// reverse) fails CI. Mirrors [`ConditionalCapability`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeConfinement {
    pub command: &'static str,
    /// The wire argument that must be present and non-null.
    pub argument: &'static str,
    /// What the confinement buys, and — honestly — what it does not.
    pub reason: &'static str,
}

pub const SCOPE_CONFINEMENTS: &[ScopeConfinement] = &[
    ScopeConfinement {
        command: "pause_execution",
        argument: "projectId",
        reason: "null projectId sweeps every project via project_repo.get_all(); \
                 confines the task-transition sweep, NOT the global pause flag — \
                 discharged by require_explicit_project_scope",
    },
    ScopeConfinement {
        command: "stop_execution",
        argument: "projectId",
        reason: "null projectId sweeps every project via project_repo.get_all(); \
                 confines the task-transition sweep, NOT the global pause flag — \
                 discharged by require_explicit_project_scope",
    },
];

pub const DECLARED_MEMBERSHIPS: &[(&str, &str)] = &[
    ("approve_permission_request", "authorizes-live-tool-call"),
    ("resolve_user_question", "steering-question"),
    // Declared, precisely BECAUSE the detectors are silent on it. The P-17b negative
    // suite is generated from detector output, so a spawn-free command that steers a
    // live agent would otherwise never be proved unreachable from a default pairing —
    // the one class of member the generator cannot find by itself.
    ("send_remote_chat_message", "steers-live-agent-turn"),
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
            RiskClass::Denied,
            DELETE,
            "deletes a durable entity",
        ));
    }
    MODULE_DEFAULTS
        .iter()
        .find(|entry| entry.module == module)
        .map(|entry| entry.policy)
}
