//! Exhaustive capability policy for the live Tauri command census.
//!
//! The ledger is deliberately module-defaulted with narrow command overrides. Command modules
//! are RalphX's established risk boundary, so this records the actual audit method instead of
//! pretending 500+ independent judgements were made. Tests expand these policies against
//! `commands/registry.rs`; an unknown module or duplicate command fails the census gate.

use ralphx_remote_protocol::{AuditRefusalReason, Capability, RiskClass};

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
const CONTENT_AND_SEEDS: &[Capability] = &[
    Capability::MutatesAgentConsumedContent,
    Capability::SeedsSpawnTriggeringState,
];
const PROCESS: &[Capability] = &[Capability::SpawnsProcess];
/// The process floor PLUS retained detector-(b) evidence. PR 3.1-b batch 14: a row may be
/// foreclosed by a launch and still be a proof-class arming writer, and erasing the weaker tag
/// to record the stronger one would delete evidence three tests depend on.
const PROCESS_AND_SEEDS: &[Capability] = &[
    Capability::SpawnsProcess,
    Capability::SeedsSpawnTriggeringState,
];
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

/// A command dropped from its conservative module default to `Read` by a hand audit.
///
/// The reason is mandatory and is asserted non-placeholder by the read-reclassification pins:
/// dropping a row below the module default is only ever licensed by an audit that found no
/// write, never by detector silence, so the row has to carry the finding that bought it.
const fn read_audit(command: &'static str, reason: &'static str) -> CommandOverride {
    CommandOverride {
        command,
        policy: policy(RiskClass::Read, NONE, reason),
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
    agent_default("agent_conversation_mute_commands"),
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
    denied_default(
        "database_maintenance_commands",
        PATH,
        "host database file maintenance (stats read + compaction marker) operates on this Mac's SQLite files",
    ),
    elevated_default(
        "diagnostic_commands",
        PROCESS,
        "diagnostics may spawn provider CLIs",
    ),
    elevated_default("diff_commands", PROCESS, "diff getters may spawn git"),
    agent_default("remote_diff_commands"),
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
    // Same construction and the same conservative default as `remote_chat_commands` below: the
    // module takes no AppHandle/ExecutionState/ChatService, so it cannot terminate anything, but
    // a future member must still earn its own row rather than inherit the stop pair's.
    agent_default("remote_agent_stop_commands"),
    agent_default("remote_attachment_commands"),
    // The module is spawn-free by construction (no AppHandle, no ExecutionState, no
    // ChatService), but the default stays conservative: a future member must earn a
    // narrower row rather than inherit one.
    agent_default("remote_chat_commands"),
    agent_default("remote_queue_commands"),
    // Same construction and the same conservative default as `remote_chat_commands`: the module
    // takes no AppHandle/ExecutionState/ChatService, so it cannot spawn, but a future member must
    // still earn its own row rather than inherit the start command's.
    // Same construction and the same conservative default: the continuation module takes no
    // AppHandle/ExecutionState/ChatService, so it cannot spawn, but a future member must still
    // earn its own row rather than inherit the message command's.
    agent_default("remote_conversation_message_commands"),
    agent_default("remote_conversation_start_commands"),
    // Same construction and the same conservative default as `remote_chat_commands`: the
    // module cannot spawn (no AppHandle, no ExecutionState, no ChatService), but a future
    // member must still earn its own row rather than inherit a narrow one.
    agent_default("remote_transcript_commands"),
    agent_default("remote_mcp_policy_commands"),
    // Same construction and the same conservative default again: the module cannot spawn (no
    // AppHandle, no ExecutionState, no chat service), but a future member must still earn its
    // own row rather than inherit the shell reads' narrow one.
    agent_default("remote_workspace_commands"),
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
    agent_default("remote_question_commands"),
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
    read_row(
        "list_ticketing_providers",
        "audited local settings-repository reads; response summaries omit token_secret_ref and no provider call is made",
    ),
    read_row(
        "list_ticketing_containers",
        "audited outbound provider container call spends the host credential; the credential is resolved host-side and does not cross the wire",
    ),
    CommandOverride {
        command: "list_ticketing_columns",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "syncs provider statuses into the local ticketing status catalog, changing local catalog rows",
        ),
    },
    read_row(
        "list_ticketing_status_catalog",
        "audited local status-catalog repository read; no sync, provider call, credential reference, or write",
    ),
    CommandOverride {
        command: "refresh_ticketing_status_catalog",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "fetches provider statuses and changes the local ticketing status catalog",
        ),
    },
    CommandOverride {
        command: "update_ticketing_status_presentation",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "changes display order, color, visibility, or terminal presentation in the local ticketing status catalog without an outbound provider write",
        ),
    },
    read_row(
        "list_tickets",
        "audited outbound provider ticket-list call spends the host credential; the credential is resolved host-side and does not cross the wire",
    ),
    read_row(
        "list_ticket_filter_options",
        "audited outbound provider filter-options call spends the host credential; the credential is resolved host-side and does not cross the wire",
    ),
    read_row(
        "get_ticket_detail",
        "audited outbound provider ticket-detail call spends the host credential; the credential is resolved host-side and does not cross the wire",
    ),
    read_row(
        "list_ticket_transitions",
        "audited outbound provider transition-list call spends the host credential; the credential is resolved host-side and does not cross the wire",
    ),
    read_row(
        "get_ticket_associations",
        "audited local link-repository and persisted PR-branch-summary reads; no provider call or credential reference",
    ),
    read_row(
        "get_conversation_ticket",
        "audited local conversation-link repository reads; response contains ticket identity only and no credential reference",
    ),
    read_row(
        "refresh_tickets",
        "audited capability-free clock response; validates provider and returns now_string without state, network, or credential access",
    ),
    CommandOverride {
        command: "transition_ticket_status",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "changes the ticket workflow status on the provider",
        ),
    },
    CommandOverride {
        command: "assign_ticket",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "changes the ticket assignee on the provider to the credential owner",
        ),
    },
    CommandOverride {
        command: "clear_ticket_assignee",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "clears the ticket assignee on the provider",
        ),
    },
    CommandOverride {
        command: "add_ticket_comment",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "adds a comment to the ticket on the provider",
        ),
    },
    CommandOverride {
        command: "set_ticket_labels",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "replaces the ticket labels or tags on the provider",
        ),
    },
    read_row(
        "list_ticket_labels",
        "audited outbound provider label-list call spends the host credential; the credential is resolved host-side and does not cross the wire",
    ),
    process_refusal(
        "start_ralphx_work_from_ticket",
        "detector-c, hand-traced: AgentConversationStartService::start reaches the agent conversation process launch chain",
    ),
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
    // PR 3.1-b batch 14 raised this row from AgentControl to the process floor. It is the one
    // ratchet member that ALREADY had an override, so it is amended in place rather than
    // duplicated — a second row would never be reached by `policy_for` and the command would
    // have stayed unclassified while looking handled.
    //
    // The detector-(b) evidence tag is KEPT alongside `SpawnsProcess`, deliberately: R5-H1 names
    // this command as a proof-class detector-(b) writer and three tests pin that tag, so
    // dropping it to buy the floor would delete real evidence. `Elevated` admits both, and
    // `v1_resolution` returns `HostDeniedSpawnsProcess` on the process capability regardless of
    // what else the row carries — the stronger, cheaper-to-verify statement wins without the
    // weaker one being erased.
    CommandOverride {
        command: "set_agent_conversation_workspace_auto_publish",
        policy: policy(
            RiskClass::Elevated,
            PROCESS_AND_SEEDS,
            "detector-c: resolve_agent_workspace_pr_automation_target -> \
             ensure_linked_plan_branch_agent_worktree -> GitService::get_current_branch. It \
             ALSO arms auto_publish_enabled for the auto-publish freshness scan (detector b, \
             tag retained), but the process launch is what forecloses it at every v1 scope",
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
    // Same dual-evidence shape as `set_agent_conversation_workspace_auto_publish`: the write
    // itself is a clean fail-closed repo update (detector b — it arms the per-conversation
    // Auto Review & Fix override the auto-review spawner consumes), but the response
    // projection goes through `agent_workspace_response_for_state`, whose repair-recovery
    // arm resolves CLI paths — the process floor forecloses it at every v1 scope. A future
    // spawn-free twin can serve it via the without-repair-recovery builder.
    CommandOverride {
        command: "set_agent_conversation_workspace_review_automation",
        policy: policy(
            RiskClass::Elevated,
            PROCESS_AND_SEEDS,
            "detector-c: agent_workspace_response_for_state -> repair recovery -> CLI path \
             resolution; ALSO detector-b: arms the Auto Review & Fix override consumed by \
             the auto-review spawner (tag retained)",
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
            "content-surface, declared membership steers-live-agent-turn: queues a role-pinned \
             user turn for a run that is already live; \
             detector-silent on (a), (b) and (c) — it arms no scheduler, resolves no CLI path, \
             and refuses when no live run would drain the row, so a message can never be \
             persisted as sent yet delivered to nobody. The role is pinned to \"user\" at \
             dispatch, so a remote client cannot forge an orchestrator speaker label",
        ),
    },
    // Reviewed, not inherited. Seeds a start-intent row the host-owned dispatcher loop consumes
    // to spawn — honestly `SeedsSpawnTriggeringState`, the same capability inject_task /
    // resume_automation / finalize_automation carry — PLUS `MutatesAgentConsumedContent` for the
    // first-turn content it seeds. Detector-silent on (a)/(c); detector (b) flags it mechanically
    // via the `remote-conversation-start` surface row, so no declared-membership compensation is
    // needed. The unknown-model pass-through of the local start path is deliberately NOT reused.
    CommandOverride {
        command: "request_remote_agent_conversation_start",
        policy: policy(
            RiskClass::AgentControl,
            CONTENT_AND_SEEDS,
            "seeds-spawn-triggering-state: persists a host-validated known-mode start intent a \
             host loop later spawns; validates mode/provider/model/project fail-closed and rejects unknown \
             models rather than passing them to CLI argv; resolves no CLI path and arms no \
             scheduler in-band",
        ),
    },
    CommandOverride {
        command: "get_remote_conversation_start_request",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of one start-intent row; no spawn carrier; propagates read errors",
        ),
    },
    // Reviewed, not inherited. The CONTINUATION half of remote chat send (WP1): removes the
    // one-shot behaviour `send_remote_chat_message` alone left behind. Seeds a message-intent row
    // the host-owned dispatcher loop consumes to send — honestly `SeedsSpawnTriggeringState`,
    // the same capability inject_task / resume_automation carry — PLUS
    // `MutatesAgentConsumedContent` for the turn content it seeds. Detector-silent on (a)/(c);
    // detector (b) flags it mechanically via the `remote-conversation-message` surface row.
    // The unknown-model pass-through of the local send path is deliberately NOT reused, and the
    // command REFUSES when a run is already live so it can never double a turn.
    CommandOverride {
        command: "request_remote_agent_conversation_message",
        policy: policy(
            RiskClass::AgentControl,
            CONTENT_AND_SEEDS,
            "seeds-spawn-triggering-state, declared membership seeds-agent-turn-for-idle-conversation: \
             persists a continuation intent a host loop later sends \
             through the provider-session resume seam; validates conversation ownership, \
             archival, run liveness, provider and model fail-closed and rejects unknown models \
             rather than passing them to CLI argv; has no role field to forge; resolves no CLI \
             path and arms no scheduler in-band",
        ),
    },
    CommandOverride {
        command: "get_remote_conversation_message_request",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of one message-intent row; no spawn carrier; propagates read errors",
        ),
    },
    CommandOverride {
        command: "request_remote_queued_message_send",
        policy: policy(
            RiskClass::AgentControl,
            SEEDS_STATE,
            "seeds-spawn-triggering-state: persists an id-only queued SEND-NOW intent; the host dispatcher alone resolves the payload and executes the kill-and-launch seam",
        ),
    },
    CommandOverride {
        command: "get_remote_queued_message_send_request",
        policy: policy(RiskClass::Read, NONE, "pure repository read of one queued SEND-NOW intent; propagates missing and read failures distinctly"),
    },
    // --- WP2: the spawn-free STOP pair.
    //
    // `stop_agent` itself is `host-denied-spawns-process` (AppChatService::stop_agent reaches
    // `Command::new(resolve_pkill_cli_path())`), and the process floor is absolute. But that is a
    // HYGIENE refusal, not a risk one: stopping is authority-REDUCING, the same direction as
    // `pause_execution`/`stop_execution`. So the brake is redesigned rather than relaxed — this
    // command persists an intent and a host-owned dispatcher holds the pkill path — and it is
    // classified `Operate` so the DEFAULT "viewer with brakes" pairing can halt a runaway agent
    // without ever being granted `ui:agent`. The gap between this row and its `stop_agent`
    // sibling is recorded in AUTHORITY_REDUCING_EXEMPTIONS, not asserted in a comment.
    //
    // It carries NO capability, and that is checkable rather than claimed: `class_permits`
    // admits none at `Operate`. It seeds no spawn-triggering state (the loop that reads the row
    // TERMINATES processes; it starts nothing), so it takes no `SeedsSpawnTriggeringState` and
    // no `SPAWN_TRIGGERING_STATE_SURFACE` row — see the module doc for why the surface table
    // would have been a false entry.
    CommandOverride {
        command: "get_remote_mcp_catalog",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of one coherent host-built MCP catalog snapshot; no provider readiness or catalog discovery carrier",
        ),
    },
    CommandOverride {
        command: "list_remote_message_attachments",
        policy: policy(
            RiskClass::Read,
            NONE,
            "audited metadata-only attachment repository read; propagates read errors and projects filename, MIME type, size, and identity while omitting the host file path",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_workspace_change_summary",
        policy: policy(
            RiskClass::Read,
            NONE,
            "snapshot-only in-memory read of a host-captured workspace change summary; no DiffService, GitService, or CLI resolver carrier",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_workspace_review",
        policy: policy(
            RiskClass::Read,
            NONE,
            "snapshot-only in-memory read of host-captured workspace changes and commits; no DiffService, GitService, or CLI resolver carrier",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_workspace_file_diff",
        policy: policy(
            RiskClass::Read,
            NONE,
            "snapshot-only in-memory read of one host-captured workspace file diff; no DiffService, GitService, or CLI resolver carrier",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_workspace_commit_file_diff",
        policy: policy(
            RiskClass::Read,
            NONE,
            "snapshot-only in-memory read of one host-captured commit file diff; no DiffService, GitService, or CLI resolver carrier",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_workspace_cumulative_file_diff",
        policy: policy(
            RiskClass::Read,
            NONE,
            "snapshot-only in-memory read of one host-captured cumulative file diff; no DiffService, GitService, or CLI resolver carrier",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_workspace_file_diff_page",
        policy: policy(
            RiskClass::Read,
            NONE,
            "snapshot-only in-memory read keyed by host-captured file page range and ref scope; no DiffService, GitService, or CLI resolver carrier",
        ),
    },
    CommandOverride {
        command: "request_remote_agent_stop",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing brake: persists a conversation-scoped stop intent a host-owned \
             dispatcher drains; names no pid/run/process, resolves no CLI path, arms nothing, and \
             dedupes per conversation so a second tap joins the in-flight brake",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_stop_request",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of one stop-intent row; no spawn carrier; propagates read errors",
        ),
    },
    // --- WP5a: the spawn-free MODE SWITCH pair.
    //
    // `switch_agent_conversation_mode` is `host-denied-spawns-process` (see its `process_refusal`
    // row: the body reaches `GitService::ref_exists` and the publish path's
    // `inspect_repository_capability` -> `ensure_git_worktree`). Combined with the start intent
    // host-pinning `mode` to "chat", that made every remote conversation permanently chat-only.
    //
    // The redesign is the same one WP1/WP2 used: persist an intent, let a host-owned dispatcher
    // hold the denied path. Unlike the stop brake this is NOT authority-reducing — a switch into
    // Edit/Plan prepares a worktree a later agent process runs in — so it is classified
    // `AgentControl` with an honest `SeedsSpawnTriggeringState` and takes a
    // `SPAWN_TRIGGERING_STATE_SURFACE` row, exactly like the start and continuation intents. It
    // does NOT take `MutatesAgentConsumedContent`: it seeds no turn content, only a mode.
    CommandOverride {
        command: "request_remote_agent_conversation_mode_switch",
        policy: policy(
            RiskClass::AgentControl,
            SEEDS_STATE,
            "seeds-spawn-triggering-state, declared membership prepares-workspace-for-later-agent-run: \
             persists a target-mode intent a host loop later applies through \
             `switch_agent_conversation_mode_for_state`, whose REJECT policy keeps the \
             process-terminating stop path out of the dispatcher; validates conversation \
             ownership, archival, mode validity and run liveness fail-closed; carries no \
             base/branch/runtime-override field to aim workspace preparation with; resolves no \
             CLI path and arms no scheduler in-band",
        ),
    },
    CommandOverride {
        command: "get_remote_conversation_mode_switch_request",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of one mode-switch-intent row; no spawn carrier; propagates read errors",
        ),
    },
    CommandOverride {
        command: "request_remote_execution_resume",
        policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership resumes-execution-through-host-dispatcher: persists a validated execution-resume intent; spawn_remote_resume_dispatchers is the sole spawner"),
    },
    CommandOverride {
        command: "request_remote_task_resume",
        policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership resumes-task-through-host-dispatcher: persists a validated paused-task intent; spawn_remote_resume_dispatchers is the sole spawner"),
    },
    CommandOverride {
        command: "request_remote_task_restart",
        policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership restarts-task-through-host-dispatcher: persists a validated stopped-or-failed task intent; spawn_remote_resume_dispatchers is the sole spawner"),
    },
    CommandOverride {
        command: "request_remote_group_resume",
        policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership resumes-task-group-through-host-dispatcher: persists a validated group intent; spawn_remote_resume_dispatchers is the sole spawner"),
    },
    CommandOverride {
        command: "request_remote_recovery_prompt_resolution",
        policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership resolves-recovery-through-host-dispatcher: persists a status-and-live-marker validated recovery intent; Restart may execute entry actions and Failed+Restart deletes worktree/branch before resetting retry authority; spawn_remote_resume_dispatchers is the sole dispatcher"),
    },
    CommandOverride { command: "get_remote_execution_resume_request", policy: policy(RiskClass::Read, NONE, "pure repository read of one execution-resume intent; propagates read errors") },
    CommandOverride { command: "get_remote_task_action_request", policy: policy(RiskClass::Read, NONE, "pure repository read of one task-action intent; propagates read errors") },
    CommandOverride { command: "request_remote_plan_approval", policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership approves-plan-through-host-dispatcher: persists a validated plan-approval intent; host approval can fan a Codex complexity assessor when tasks_enabled; spawn_remote_resume_dispatchers is the sole dispatcher") },
    CommandOverride { command: "get_remote_plan_approval_request", policy: policy(RiskClass::Read, NONE, "pure repository read of one plan-approval intent; propagates read errors") },
    CommandOverride { command: "request_remote_plan_artifact_edit", policy: policy(RiskClass::AgentControl, MUTATES_CONTENT, "content-surface: persists a host-applied plan edit intent; expected_version is checked at request and claim to prevent silent clobber, while caller session and agent mutation provenance are host-forced absent") },
    CommandOverride { command: "get_remote_plan_edit_request", policy: policy(RiskClass::Read, NONE, "pure repository read of one plan-edit intent; propagates missing and read failures distinctly") },
    CommandOverride { command: "request_remote_ideation_finalize_decision", policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership records accept-or-reject finalize intent; host accept creates tasks and arms the ready scheduler; spawn_remote_resume_dispatchers is the sole dispatcher") },
    CommandOverride { command: "get_remote_ideation_finalize_request", policy: policy(RiskClass::Read, NONE, "pure repository read of one finalize-decision intent; propagates read errors") },
    CommandOverride { command: "request_remote_automation_run", policy: policy(RiskClass::AgentControl, SEEDS_STATE, "seeds-spawn-triggering-state, declared membership runs-automation-now-through-host-dispatcher: persists a validated run-now or retry-judge intent; spawn_remote_resume_dispatchers is the sole dispatcher") },
    CommandOverride { command: "get_remote_automation_run_request", policy: policy(RiskClass::Read, NONE, "pure repository read of one automation-run intent; propagates read errors") },
    CommandOverride { command: "request_remote_automation_draft", policy: policy(RiskClass::AgentControl, CONTENT_AND_SEEDS, "seeds-spawn-triggering-state and content-surface: persists a validated automation-draft intent with a pre-allocated automation id; spawn_remote_resume_dispatchers is the sole dispatcher") },
    CommandOverride { command: "get_remote_automation_draft_request", policy: policy(RiskClass::Read, NONE, "pure repository read of one automation-draft intent; propagates read errors") },
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
    // Renamed from `delete_task_proposal` in Wave D: the body calls `archive_proposal_impl`
    // and has always archived rather than deleted, so the old name only ever earned it the
    // `delete_` prefix floor. Same content capability as its update sibling — archiving a
    // proposal changes what a worker subsequently reads.
    CommandOverride {
        command: "archive_task_proposal",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "content-surface: archives a worker-consumed task proposal",
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
    //   `get_remote_execution_status` is the spawn-free twin that omits that cleanup path.
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
    CommandOverride {
        command: "pause_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "leaving Executing/ReExecuting reaches the normal exit auto-commit path and can invoke Git",
        ),
    },
    CommandOverride {
        command: "block_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "exiting an agent-active state decrements capacity and calls try_schedule_ready_tasks through the attached scheduler, which can launch queued work",
        ),
    },
    CommandOverride {
        command: "stop_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "leaving Executing/ReExecuting reaches the normal exit auto-commit path and can invoke Git",
        ),
    },
    CommandOverride {
        command: "pause_tasks_in_group",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "bulk-pauses an attacker-chosen group; leaving Executing/ReExecuting reaches the normal exit auto-commit path and can invoke Git",
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
        command: "get_agent_run_attribution",
        policy: policy(
            RiskClass::Read,
            NONE,
            "bounded agent_run_repo lookup of one persisted attribution row; no spawn, no writes",
        ),
    },
    CommandOverride {
        command: "get_agent_run_attributions",
        policy: policy(
            RiskClass::Read,
            NONE,
            "batched (max 100) agent_run_repo lookup of persisted attribution rows; no spawn, no writes",
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
    // escape hatches (`get_agent_message_tool_call_detail` and its timeline twin) were
    // deliberately NOT registered by batch 4 because they were fail-open. WP3 fixed the
    // fail-open at its source and registers them below.
    CommandOverride {
        command: "get_remote_agent_conversation",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of a conversation and its messages; no wake, no spawn; propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_remote_agent_conversation_workspace",
        policy: policy(
            RiskClass::Read,
            NONE,
            "recovery-free persisted workspace read via agent_workspace_response_without_repair_recovery_for_state; blanks host paths and propagates read errors",
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

    // ---- WP3 — the tool-call-detail pair, released from the fail-open refusal ---------------
    //
    // Batch 4 refused both under `AuditRefusalReason::FailOpenUntilFixed`: the shared
    // `load_delegated_tool_runtime_snapshot` helper applied `.ok().flatten()` to five repository
    // reads, so a repository outage served the STALE persisted tool result as though it were
    // the delegate's current live state. The manifest's own rule is that a repaired error path
    // is not a registration decision on its own, so both rows carry the per-command audit that
    // clears them, not just the fix.
    //
    // The audit: each command is `&AppState` plus repository reads — no `AppHandle`, no
    // `ExecutionState`, no `ChatService`, no route through `agent_workspace_response_for_state`.
    // Every read now propagates through one `?` seam
    // (`unified_chat_commands/mod.rs::load_delegated_tool_runtime_snapshot`), with `Ok(None)`
    // reserved for genuine absence (missing session row, non-delegation conversation,
    // cross-conversation run) — the distinction pinned by
    // `timeline_tool_call_detail_fails_closed_on_a_delegated_repository_outage`.
    //
    // Tier: `ui:read`, matching the registered `get_remote_agent_conversation` twin, which
    // already serves transcript text and tool blocks at that scope. These two return the
    // UN-truncated payload of a block that twin already surfaces truncated; the content class
    // is identical, only the truncation differs.
    //
    // The same seam fix discharges follow-up A3/L2: the three transcript reads above share
    // `load_delegated_tool_runtime_snapshot`, so their "propagates read errors" reason is now
    // true for the delegated-tool reads too, which it was not when it was written.
    read_audit(
        "get_agent_message_tool_call_detail",
        "WP3 audit: `chat_message_repo` read plus delegated-run reconciliation, all errors          propagated (`load_delegated_tool_runtime_snapshot` now returns `AppResult`); no          AppHandle/ExecutionState/ChatService, no repository write",
    ),
    read_audit(
        "get_agent_timeline_item_tool_call_detail",
        "WP3 audit: `chat_timeline_repo` read plus the same propagating delegated-run          reconciliation; no AppHandle/ExecutionState/ChatService, no repository write",
    ),
    // -----------------------------------------------------------------------------------
    // Batch 5 — the conversation-LIST seam split. Completes PR 3.2's read surface.
    //
    // These carry conversation METADATA only (titles, counts, timestamps, runtime
    // attribution) — no message text. The content step-up this batch does NOT repeat is the
    // transcript one above.
    //
    // Every read on this path propagates: `filter_agent_list_visible_conversations`,
    // `agent_conversation_responses_for_state`, and
    // `latest_conversation_runtime_attribution` each `?` their repository errors. That was
    // checked by hand rather than assumed, because a `Vec`-returning read is exactly the
    // fail-open shape batch 4 refused four times.
    // -----------------------------------------------------------------------------------
    // -----------------------------------------------------------------------------------
    // The execution-settings write (`remote_execution_settings_commands`).
    //
    // `update_execution_settings` is Elevated because it reaches two spawn sinks:
    // `schedule_ready_tasks_for_project` (launches queued work when the cap rises) and
    // `PendingSessionDrainService` (builds a chat service, which spawns a provider CLI). The
    // split below persists and syncs the in-process caps and reaches NEITHER, so the
    // registrable class is the one the remaining authority earns.
    //
    // AgentControl, not Operate: persisting a higher cap seeds state a background scheduling
    // pass turns into a spawn. Classification traces downstream authority, not immediate
    // action, so a write whose only effect is a database row is still AgentControl when a
    // loop consumes that row. Requires `ui:agent`, which is granted at pairing and revocable
    // per device.
    // -----------------------------------------------------------------------------------
    // -----------------------------------------------------------------------------------
    // WP4 (a) — the rows batch 14 refused on a transport shape that does not exist.
    //
    // Each was left inheriting a conservative module default because the batch never got past
    // the (false) error-contract blocker to audit the body. These four are the reviewed rows;
    // the four `task_step_commands` status writes already carried their own detector-d rows.
    // -----------------------------------------------------------------------------------
    CommandOverride {
        command: "list_conversation_folder_references",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure `SELECT ... WHERE removed_at IS NULL` over the folder-reference repository; takes no path, writes nothing, propagates read errors. Discloses the stored host folder_path, on the same owner ruling that lets `list_remote_projects` carry working_directory: the paired device is the user's own machine holding ui:read on their own host",
        ),
    },
    CommandOverride {
        command: "remove_conversation_folder_reference",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "soft-deletes ONE folder reference, scoped by both folder_reference_id and conversation_id, and fails closed with NotFound on a missing row; takes no path and reaches no spawn sink. AgentControl because the reference list is read at spawn time to build the MCP filesystem roots, so removing one narrows a future agent's reach — the authority-REDUCING half of the pair whose adding half stays deferred on the missing project-root allowlist",
        ),
    },
    CommandOverride {
        command: "reorder_task_steps",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "reorders a task's steps in one transaction scoped `WHERE id = ?2 AND task_id = ?3`, so a foreign step id is a no-op rather than a cross-task write; propagates every repository error. Not a content writer — it moves sort_order and no step body — so it does not carry MutatesAgentConsumedContent the way its four status siblings do",
        ),
    },
    CommandOverride {
        command: "abort_seeded_agent_conversation",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "cancels a NEVER-STARTED seeded conversation and the resources minted while preparing its first send. Guarded fail-closed: it refuses with SeededAgentConversationAlreadyStarted if the conversation has any message, any run, or any provider/claude session id, so it can never reach a conversation with history; every step propagates by `?` and it launches nothing",
        ),
    },
    CommandOverride {
        command: "update_remote_execution_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "persists execution settings and syncs the in-process caps; reaches neither the scheduler kick nor the ideation drain, but a raised cap seeds work a later scheduling pass can launch",
        ),
    },
    CommandOverride {
        command: "get_remote_execution_status",
        policy: policy(
            RiskClass::Read,
            NONE,
            "spawn-free status derivation from DB halt mode plus in-memory registry/atomics; propagates read errors, performs no process inspection, and makes no runtime writes",
        ),
    },
    // -----------------------------------------------------------------------------------
    // The workspace shell reads (`remote_workspace_commands`).
    //
    // Both are splits, not reclassifications. `list_projects` is Elevated because
    // `project_response` runs `inspect_repository_capability` over the project's working
    // directory; `get_agent_provider_settings` is Denied because its refresh path probes
    // provider CLIs and its response carries per-provider CLI/model detail. The rows below
    // cover the projections that reach neither sink — checked against the source, not assumed,
    // because a Vec-returning read is exactly the fail-open shape this ledger has refused
    // before. Both propagate their repository errors with `?`.
    // -----------------------------------------------------------------------------------
    CommandOverride {
        command: "list_remote_projects",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository reads of project rows plus stored repository-capability snapshots; performs no live inspection and propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_remote_project",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository reads of one project row plus its stored repository-capability snapshot through the same projection as `list_remote_projects`; performs no live inspection and propagates read errors",
        ),
    },
    CommandOverride {
        command: "get_remote_provider_readiness",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read reduced to two scalars; no CLI probe, no provider identity, model, path, or credential surface",
        ),
    },
    CommandOverride {
        command: "list_remote_agent_providers",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read projecting stored provider enablement, default flag, and default model/effort names; no CLI probe, no path, no credential, no process-configuration surface",
        ),
    },
    CommandOverride {
        command: "list_remote_agent_conversations",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of a context's conversation metadata; no spawn carrier; propagates read errors",
        ),
    },
    CommandOverride {
        command: "list_remote_queued_agent_messages",
        policy: policy(
            RiskClass::Read,
            NONE,
            "spawn-free AppState-only queue read; validates a live Project conversation, propagates durable repository errors, merges durable-first, and filters hidden recovery rows",
        ),
    },
    // Named `cancel_`, not `delete_`, deliberately: the P-17c deny surface blanket-denies the
    // `delete_` prefix as the entity-deletion floor (owner decision D2). This op is not an
    // entity deletion — it withdraws a not-yet-consumed queued turn, the same
    // authority-REDUCING direction as request_remote_agent_stop, and the twin-naming rule
    // says a twin is named for what its closure does. If D2 is ever revisited, this row is
    // the place to re-argue the boundary.
    CommandOverride {
        command: "cancel_remote_queued_agent_message",
        policy: policy(
            RiskClass::Operate,
            NONE,
            "authority-reducing queue removal after live Project-conversation validation; durable-first deletion prevents restart resurrection and cannot add or dispatch agent-consumed content",
        ),
    },
    CommandOverride {
        command: "list_remote_agent_conversations_page",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pure repository read of a conversation-list page; no spawn carrier; propagates read errors",
        ),
    },
    CommandOverride {
        command: "list_remote_agent_sidebar_conversations",
        policy: policy(
            RiskClass::Read,
            NONE,
            "Agents-sidebar inbox read over conversation/workspace/run repositories, hydrated through the recovery-free workspace seam so it schedules NO PR-supervision recovery and reaches no CLI resolver; the host worktree_path is blanked at the facade; propagates read errors. The local list_agent_sidebar_conversations stays host-denied because it DOES schedule recovery",
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
            RiskClass::AgentControl,
            AGENT,
            "bulk-terminalizes an attacker-chosen group and execution exits reach auto-commit, which invokes Git",
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
    // PR 3.1-b batch 10 — the batch-9 shadowing gap, closed in the ONE authoritative row.
    //
    // Batch 9 measured this command as the fifteenth detector-(c) refusal: answering a live gate
    // resumes the agent turn, so its closure resolves the git, node and Codex CLIs exactly as the
    // thirteen corrected rows did. It could not append a `process_refusal` for it, because
    // `policy_for` is a FIRST-MATCH lookup and this row already existed — the appended row was
    // silently shadowed, and the duplicate-override assert plus the new detector-(c) gate both
    // fired on the shadowed copy. Batch 9 recorded the gap rather than half-fixing it.
    //
    // The fix is not a second row. It is this row, corrected in place, so there is exactly one
    // authoritative statement about the command.
    //
    // The declared membership is PRESERVED and is a substring of the reason. That is deliberate:
    // the membership was never the false part. "steering-question" is a true claim about what the
    // command does and it is what keeps the command in the P-17b `ui:agent` negative suite and in
    // its ANCHORS list. What was false was the CLASS — `AgentControl`/`AGENT` understated a
    // closure that resolves three CLI binaries. Correcting the class is authority-INCREASING and
    // strictly strengthens the guarantee: the command moves from "unreachable from a default
    // pairing" to "unreachable at every scope", which
    // `manifest_classified_commands_stay_unreachable_at_every_scope` proves.
    //
    // `exemptions_and_declared_memberships_are_exact` was pinning this row's reason as VERBATIM
    // equal to `DECLARED_MEMBERSHIPS[1].1`, which is what made the correction a contract change.
    // Batch 10 relaxes that pin to the `contains` form its `resolve_permission_request` sibling
    // already used — the membership must still be carried, but the row may also state the finding.
    CommandOverride {
        command: "resolve_user_question",
        policy: policy(
            RiskClass::Elevated,
            PROCESS,
            "detector-c: steering-question — answering a live gate resumes the agent turn, so the \
             closure resolves resolve_git_cli_path, resolve_node_cli_path and \
             find_codex_cli_candidates; the registered resolve_remote_user_question twin omits \
             handle_accepted_plan_mode_proposal/create_chat_service/kick_runtime_handoff and \
             refuses Plan-mode acceptance fail-closed",
        ),
    },
    CommandOverride {
        command: "resolve_remote_user_question",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "declared membership: steering-question; spawn-free answer twin omits \
             handle_accepted_plan_mode_proposal, create_chat_service, and kick_runtime_handoff, \
             and refuses Plan-mode acceptance fail-closed before committing the claim",
        ),
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
    // PR 3.1-b batch 7 — census `B3`, the review/QA/merge-pipeline read cluster.
    //
    // Every row here previously resolved through its module's `agent_default`, i.e. it carried
    // "conservative-module-default: may steer or arm autonomous work" — a placeholder, not a
    // reviewed judgement. The module defaults stay `AgentControl` because `review_commands`
    // also holds the human approval/transition actions and `qa_commands` holds `retry_qa`.
    //
    // The shared structural reason, identical to batch 2's `task_commands` cluster: each body
    // is a repository (or in-memory store) query whose error is PROPAGATED — `map_err(...)?`
    // or `?` — never collapsed into an empty or default result. A read failure therefore
    // cannot be presented to a remote client as "no reviews", "no issues" or "QA never ran".
    //
    // These verdicts were taken against the call graph AFTER the same-name delegation fix in
    // this batch. Under the old graph every command in this cluster that delegates to an
    // identically-named service (`get_issue_progress`, `mark_issue_*`, …) had a closure that
    // stopped at its own body, so "detectors silent" carried no information. The one member
    // that changed verdict under the fix — `get_task_validation_summary` — is excluded below.
    CommandOverride {
        command: "get_pending_reviews",
        policy: policy(
            RiskClass::Read,
            NONE,
            "pending-review enumeration: `review_repo.get_pending` mapped to responses; \
             selects rows and starts no review",
        ),
    },
    CommandOverride {
        command: "get_review_by_id",
        policy: policy(
            RiskClass::Read,
            NONE,
            "single-review read: `review_repo.get_by_id`, an Option-returning row read whose \
             repository error propagates rather than reading as absent",
        ),
    },
    CommandOverride {
        command: "get_reviews_by_task_id",
        policy: policy(
            RiskClass::Read,
            NONE,
            "per-task review list: `review_repo.get_by_task_id` mapped to responses",
        ),
    },
    CommandOverride {
        command: "get_task_state_history",
        policy: policy(
            RiskClass::Read,
            NONE,
            "review-note history: `review_repo.get_notes_by_task_id`; reads notes already \
             written and writes none",
        ),
    },
    CommandOverride {
        command: "get_fix_task_attempts",
        policy: policy(
            RiskClass::Read,
            NONE,
            "fix-attempt count: `review_repo.count_fix_actions` rendered as a scalar",
        ),
    },
    CommandOverride {
        command: "get_task_issues",
        policy: policy(
            RiskClass::Read,
            NONE,
            "issue list: `review_issue_repo.get_open_by_task_id`/`get_by_task_id` selected by \
             a status filter; both halves propagate their repository error",
        ),
    },
    CommandOverride {
        command: "get_issue_progress",
        policy: policy(
            RiskClass::Read,
            NONE,
            "issue progress summary: `review_issue_repo.get_summary`, an aggregate read",
        ),
    },
    CommandOverride {
        command: "get_review_settings",
        policy: policy(
            RiskClass::Read,
            NONE,
            "review policy read: `review_settings_repo.get_settings`; the WRITE half \
             (`update_review_settings`) seeds spawn-triggering state and stays AgentControl",
        ),
    },
    CommandOverride {
        command: "get_qa_settings",
        policy: policy(
            RiskClass::Read,
            NONE,
            "QA settings read: clones the in-memory `AppState::qa_settings` behind a read \
             guard; the WRITE half (`update_qa_settings`) arms auto-QA and stays AgentControl",
        ),
    },
    CommandOverride {
        command: "get_task_qa",
        policy: policy(
            RiskClass::Read,
            NONE,
            "per-task QA record: `task_qa_repo.get_by_task_id` mapped to a response",
        ),
    },
    CommandOverride {
        command: "get_qa_results",
        policy: policy(
            RiskClass::Read,
            NONE,
            "QA test results: `task_qa_repo.get_by_task_id` projected to its `test_results`; \
             retries nothing and resets no result",
        ),
    },
    CommandOverride {
        command: "get_merge_pipeline",
        policy: policy(
            RiskClass::Read,
            NONE,
            "merge-pipeline projection: batched `project_repo`/`task_repo`/`plan_branch_repo`/\
             `agent_conversation_workspace_repo` reads bucketed by `InternalStatus`; every \
             repository error propagates and no merge is started, deferred or resolved",
        ),
    },
    CommandOverride {
        command: "get_merge_progress",
        policy: policy(
            RiskClass::Read,
            NONE,
            "merge-progress hydration: clones accumulated events out of the in-memory \
             `MERGE_PROGRESS_STORE`. The empty default is absence of emitted events, not a \
             swallowed error — the store read cannot fail",
        ),
    },
    CommandOverride {
        command: "get_merge_phase_list",
        policy: policy(
            RiskClass::Read,
            NONE,
            "merge phase-list hydration: clones the stored phase list out of the in-memory \
             `MERGE_PHASE_LIST_STORE`; returns `None` when nothing was emitted",
        ),
    },
    // Detector (c) finding, and the reason this batch's graph fix had to land first: the
    // validation summary delegates to `TaskValidationService::get_task_validation_summary`,
    // which calls `GitService::get_head_sha` -> `git_cmd::run(["rev-parse","HEAD"])` ->
    // `resolve_git_cli_path`. It is the `list_projects` shape exactly — a getter whose only
    // process authority is one incidental response field (`current_head_sha`, used to decide
    // whether the latest validation run still matches HEAD).
    //
    // `SpawnsProcess` is expressible only under `Elevated`, so this row is NOT registerable on
    // the v1 facade at any scope and resolves through the manifest instead of a client-local
    // reason: it is a host command the facade denies, not a command the client handles.
    //
    // The `list_projects` remedy (census §5.1 option A — cache the process-derived field and
    // read it in the getter) would apply here too and would make this `Read`. That is a code
    // change gated on the same owner call, so it stays out of this batch.
    CommandOverride {
        command: "get_task_validation_summary",
        policy: policy(
            RiskClass::Elevated,
            PROCESS,
            "resolves the git CLI through `GitService::get_head_sha` to stamp the validation \
             summary with the current HEAD sha",
        ),
    },
    // PR 3.1-b batch 7 — census `B4`, the plan / methodology / workflow read cluster.
    //
    // Same structural reason as the `B3` cluster: repository reads whose errors propagate.
    // The module defaults stay `AgentControl` because `plan_commands` also holds
    // `set_active_plan`/`clear_active_plan` and `workflow_commands` holds the writers.
    CommandOverride {
        command: "get_active_plan",
        policy: policy(
            RiskClass::Read,
            NONE,
            "active-plan read: `active_plan_repo.get` rendered as an Option<String>; selects \
             no plan and records no selection",
        ),
    },
    CommandOverride {
        command: "get_active_execution_plan",
        policy: policy(
            RiskClass::Read,
            NONE,
            "active execution-plan id read: `active_plan_repo.get_execution_plan_id`",
        ),
    },
    CommandOverride {
        command: "list_plan_selector_candidates",
        policy: policy(
            RiskClass::Read,
            NONE,
            "plan selector candidates: `ideation_session_repo.get_by_project` filtered to \
             Accepted, joined per session with `task_repo.get_by_ideation_session` and scored \
             in process; every repository error propagates",
        ),
    },
    CommandOverride {
        command: "get_methodologies",
        policy: policy(
            RiskClass::Read,
            NONE,
            "methodology list: `methodology_repo.get_all` mapped to responses",
        ),
    },
    CommandOverride {
        command: "get_active_methodology",
        policy: policy(
            RiskClass::Read,
            NONE,
            "active methodology read: `methodology_repo.get_active`; the ACTIVATE half writes \
             the active row and stays AgentControl",
        ),
    },
    CommandOverride {
        command: "get_workflows",
        policy: policy(
            RiskClass::Read,
            NONE,
            "workflow list: `workflow_repo.get_all` mapped to responses",
        ),
    },
    CommandOverride {
        command: "get_workflow",
        policy: policy(
            RiskClass::Read,
            NONE,
            "single workflow read: `workflow_repo.get_by_id`, Option-returning with the \
             repository error propagated rather than read as absent",
        ),
    },
    CommandOverride {
        command: "get_builtin_workflows",
        policy: policy(
            RiskClass::Read,
            NONE,
            "built-in workflow schemas: constructs three in-process constants and touches no \
             state at all — the command takes no `AppState`",
        ),
    },
    CommandOverride {
        command: "get_active_workflow_columns",
        policy: policy(
            RiskClass::Read,
            NONE,
            "active column set: `workflow_repo.get_default` with its error propagated, \
             falling back to the built-in RalphX columns only when no default is SET; \
             `seed_builtin_workflows` is the write half and stays AgentControl",
        ),
    },
    // PR 3.1-b batch 8 — census `B2`, the agent-conversation read cluster.
    //
    // `agent_composer_commands` defaults to `AgentControl` and STAYS there; the neighbouring
    // `unified_chat_commands` holds `send_agent_message` (the detector-(a) steer sink), the
    // workspace publish/`git push` surface, and the conversation lifecycle writes. This row is
    // reclassified individually, never by module analogy.
    //
    // The reason, verified against the body rather than inherited: a pure read whose every
    // repository error propagates via `map_err(...)?`, with no `AppHandle`, no
    // `ExecutionState` and no chat service, so a read failure cannot reach a remote client as
    // "no plans match".
    CommandOverride {
        command: "search_agent_composer_plan_references",
        policy: policy(
            RiskClass::Read,
            NONE,
            "plan-reference search: ideation sessions plus artifact resolution, ranked and              truncated to a capped limit; the resolver fail-open that once dropped sessions              silently was already removed, so a resolver outage now errors instead of              shipping a short list that looks complete",
        ),
    },
    // ---------------------------------------------------------------------------------------
    // PR 3.1-b batch 10 — reviewed rows for the arming/steering writes this batch REGISTERS.
    //
    // Each of these was on the ratchet carrying the `agent_default` placeholder
    // ("conservative-module-default: may steer or arm autonomous work"), which records that no
    // judgement was made. Batch 9 measured every one of them detector-(c) SILENT, so the absolute
    // floor does not reach them, and then declined to classify them because the finding behind
    // each refusal was arming or steering — a shape the facade demonstrably serves at `ui:agent`.
    //
    // Batch 10 did the registration audit batch 9 said they needed. Each row below states what
    // the audit FOUND, not what the module guessed. The commands whose audit came back dirty are
    // in `AUDIT_REFUSALS` instead; nothing here is registered on detector silence alone.
    //
    // Capability sets are deliberately conservative and unchanged where a reviewed row already
    // existed. `AgentControl` is the class ceiling for v1 in every case, so these capability
    // lists document WHY the class is required; they never lower it.
    // ---------------------------------------------------------------------------------------
    CommandOverride {
        command: "re_review_task_from_escalated",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "detector-a/b: guards internal_status == Escalated, optionally restores a stale \
             worktree path (errors propagated with `?`), then transitions to PendingReview, which \
             dispatches the AI reviewer. A user-initiated gate decision of precisely the shape \
             `ui:agent` exists for",
        ),
    },
    CommandOverride {
        command: "retry_qa",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "reads task_qa_repo.get_by_task_id and writes a fresh all-Pending QAResults through \
             update_results; every error propagates with `?`. Its sibling `skip_qa` is REFUSED — \
             skip writes a verdict that does not mean what its name promises, while retry writes \
             the unambiguous Pending reset",
        ),
    },
    CommandOverride {
        command: "update_qa_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "arms-auto-qa: applies only the Some(..) fields of the input to the in-memory \
             AppState::qa_settings write guard, so it can enable auto-QA. Detector-silent by \
             construction — the surface is a RwLock, not a repository — which is why it also \
             carries an explicit DECLARED_MEMBERSHIPS row. The READ half (`get_qa_settings`) is \
             already registered at Read",
        ),
    },
    CommandOverride {
        command: "set_active_project",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "arms-scheduler-quota: calls sync_quota_from_project, which writes the runtime ExecutionState \
             max_concurrent and project_ideation_max atomics that can_start_task reads. \
             Deliberately NOT declared SeedsSpawnTriggeringState: unlike its siblings \
             set_max_concurrent and update_execution_settings, this command never calls \
             schedule_ready_tasks_for_project, so it raises the ceiling without itself \
             dispatching anything. Detector-silent, hence the DECLARED_MEMBERSHIPS row",
        ),
    },
    CommandOverride {
        command: "clear_active_plan",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "authority-reducing plan clear: a single active_plan_repo.clear whose error \
             propagates through map_err. Registered where its WRITE sibling `set_active_plan` is \
             refused, and the asymmetry is the whole finding — set_active_plan additionally \
             derives an execution_plan_id behind `if let Ok(Some(ep))` and discards the follow-up \
             write with `let _ =`; clear touches execution_plan_id not at all",
        ),
    },
    CommandOverride {
        command: "seed_builtin_workflows",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "idempotent built-in workflow seed: for each of the three builtins, creates it only \
             when workflow_repo.get_by_id returns None, so re-running is a no-op returning Ok(0) \
             and a customised builtin is never overwritten; every error propagates with `?`",
        ),
    },
    CommandOverride {
        command: "start_research",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "durable row write only: builds a ResearchProcess, marks it Running and persists it \
             via process_repo.create with errors propagated. Recorded honestly — no spawn is \
             reached, transitively or otherwise, and no production consumer scans for Running \
             ResearchProcess rows, so this arms nothing today; it is registered as a guarded \
             write, NOT as a research launcher",
        ),
    },
    // ---------------------------------------------------------------------------------------
    // PR 3.1-b batch 9 ITEM 0 — the detector-(c) refusals, given the capability they actually
    // carry.
    //
    // Batches 1–8 audited each of these, found a process launch in its closure, and refused it.
    // The refusal was pinned; the ledger row was left at the `AgentControl` module default. That
    // left a command whose closure resolves a CLI binary rendering as `registerable`, which is
    // the P-11 ratchet's own definition of an unresolved name — so the audit's result was
    // invisible to the gate that exists to count it.
    //
    // Every row below was re-measured against the CURRENT graph by
    // `probe_batch9_detector_c_sink_evidence`, never inherited from a tracker's recorded verdict
    // — `reopen_issue` is the standing proof that a recorded detector verdict can be a same-name
    // artefact. The probe reports the SPECIFIC `PROCESS_LAUNCH_SINKS` resolver reached, and each
    // reason below names it. `batch9_detector_c_refusals_declare_the_capability_they_reach`
    // asserts the measurement, so a row cannot keep `SpawnsProcess` after its launch path goes
    // away.
    process_refusal(
        "get_execution_status",
        "detector-c: resolves the process-inspection CLI (resolve_tasklist_cli_path) to report \
         live execution status; get_remote_execution_status is the spawn-free read twin",
    ),
    process_refusal(
        "get_running_processes",
        "detector-c: resolves the process-inspection CLI (resolve_tasklist_cli_path)",
    ),
    process_refusal(
        "is_agent_running",
        "detector-c: the read-only registry cleanup path resolves the process-kill CLIs \
         (resolve_pkill/taskkill/tasklist) to reap dead entries",
    ),
    process_refusal(
        "get_agent_running_states",
        "detector-c: same read-only registry cleanup path, same process-kill CLI resolvers",
    ),
    process_refusal(
        "get_agent_conversation_runtime_statuses",
        "detector-c: inherits the running-states registry cleanup path and its process-kill CLI \
         resolvers",
    ),
    process_refusal(
        "is_chat_service_available",
        "detector-c: the harness capability probe resolves the Codex CLI \
         (find_codex_cli_candidates)",
    ),
    process_refusal(
        "search_agent_composer_entries",
        "detector-c: indexes project entries via Command::new(resolve_git_cli_path())",
    ),
    process_refusal(
        "get_agent_conversation_workspace_freshness",
        "detector-c: compares base against remote via resolve_git_cli_path",
    ),
    process_refusal(
        "get_agent_conversation_workspace",
        "detector-c: the workspace hydrator reaches resolve_git_cli_path, resolve_node_cli_path \
         and find_codex_cli_candidates; it also arms, but the process launch is what forecloses \
         every v1 scope; get_remote_agent_conversation_workspace is the registered recovery-free twin",
    ),
    process_refusal(
        "list_agent_conversation_workspaces_by_project",
        "detector-c: same workspace hydrator, same three CLI resolvers",
    ),
    process_refusal(
        "list_agent_sidebar_conversations",
        "detector-c: the sidebar list reaches the same hydrator and its three CLI resolvers",
    ),
    process_refusal(
        "send_agent_message",
        "detector-c: the send path resolves the git, node and Codex CLIs to run the agent turn",
    ),
    process_refusal(
        "start_agent_conversation",
        "detector-c: conversation start reaches the same three CLI resolvers as the send path",
    ),
    // `resolve_user_question` is the fifteenth measured detector-(c) refusal and is deliberately
    // NOT here. Its closure does reach `resolve_git_cli_path`, `resolve_node_cli_path` and
    // `find_codex_cli_candidates` — answering a live gate resumes the agent turn — so its
    // `AgentControl`/`AGENT` row understates it. But that row is pinned twice over by
    // `exemptions_and_declared_memberships_are_exact`, which asserts BOTH its exact class and
    // that its reason string is verbatim `DECLARED_MEMBERSHIPS[1].1`. Rewriting a declared
    // membership is a contract change, not the retroactive closure of an unclassified refusal,
    // so batch 9 recorded the finding and left the row alone. Pinned as a successor gap by
    // `batch9_records_the_declared_membership_process_launch_gap`.
    //
    // ---- PR 3.1-b batch 11 — census B4 remainder, hand-audited -----------------------------
    //
    // Every row below replaces the `agent_default("ideation_commands")` (or workflow/methodology)
    // placeholder with a reviewed reason. The reads drop to `Read`/`NONE` on the b1 precedent:
    // detector (a)/(b)/(c) all silent AND a hand audit confirming no repository write. The
    // writers stay at `AgentControl` — a silent detector never licenses dropping a writer.
    //
    // The B4 reads. All confirmed write-free by body audit, not by detector silence alone.
    read_audit(
        "get_ideation_session",
        "batch-11 audit: one `ideation_session_repo` read plus in-memory title hydration; \
         `agent_planning_session_titles` mutates the returned struct, never the repository",
    ),
    read_audit(
        "get_ideation_session_with_data",
        "batch-11 audit: same single-session read widened with proposals/dependencies; no write",
    ),
    read_audit(
        "get_ideation_agent_workspace",
        "batch-11 audit: resolves the linked workspace through \
         `resolve_agent_workspace_target_for_ideation_session` — three repository reads and a \
         pure title helper. NOT the `agent_workspace_response_for_state` hydrator, which is the \
         detector-(c) funnel that forecloses the agent-conversation twins",
    ),
    read_audit(
        "list_ideation_sessions",
        "batch-11 audit: project-scoped session list; no write",
    ),
    read_audit(
        "get_session_group_counts",
        "batch-11 audit: aggregate count query; no write",
    ),
    read_audit(
        "list_sessions_by_group",
        "batch-11 audit: paged group list; the `group` argument is checked against an allowlist \
         and rejected on miss, so it cannot widen the query",
    ),
    read_audit(
        "get_child_sessions",
        "batch-11 audit: child-session read with purpose filter; no write",
    ),
    read_audit(
        "get_latest_child_session_id",
        "batch-11 audit: single-id read; the purpose parse is `.transpose()?`, not a default",
    ),
    read_audit(
        "get_task_proposal",
        "batch-11 audit: one proposal read; no write",
    ),
    read_audit(
        "list_session_proposals",
        "batch-11 audit: session-scoped proposal list; no write",
    ),
    read_audit(
        "get_proposal_dependencies",
        "batch-11 audit: dependency edge read; no write",
    ),
    read_audit(
        "get_proposal_dependents",
        "batch-11 audit: reverse dependency edge read; no write",
    ),
    read_audit(
        "get_task_blockers",
        "batch-11 audit: blocker read; no write",
    ),
    read_audit(
        "get_blocked_tasks",
        "batch-11 audit: blocked-task read; no write",
    ),
    read_audit(
        "get_tasks_disable_impact",
        "batch-11 audit: `TasksFeatureToggleService::get_disable_impact` aggregates counts and \
         returns them; it is the read half of the toggle pair and never emits or persists. Its \
         writing sibling `set_tasks_feature_enabled` is a detector-(c) refusal below",
    ),
    read_audit(
        "get_ideation_settings",
        "batch-11 audit: settings read; the repo maps only `QueryReturnedNoRows` to the default \
         and propagates every other error",
    ),
    read_audit(
        "get_ideation_effort_settings",
        "batch-11 audit: effort read; the `unwrap_or_else` fallbacks fire on an absent row, \
         never on a swallowed `Err`",
    ),
    read_audit(
        "get_ideation_model_settings",
        "batch-11 audit: model read; same absent-row-not-error fallback shape as the effort read",
    ),
    read_audit(
        "get_agent_lane_settings",
        "batch-11 audit: lane settings read; both branches propagate with `map_err(..)?`",
    ),
    //
    // The B4 writers. Registered at `ui:agent` on a body audit, NOT dropped to Read.
    CommandOverride {
        command: "update_ideation_session_title",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: single `ideation_sessions` title write, read back before return; \
             the only discard is the post-commit `app.emit`",
        ),
    },
    CommandOverride {
        command: "reorder_proposals",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: the reorder is ONE `UPDATE .. SET sort_order = CASE ..` statement, \
             so there is no half-reordered mid-loop state; failure propagates",
        ),
    },
    CommandOverride {
        command: "assess_proposal_priority",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: pure in-process scoring (dependency/critical-path/keyword factors) \
             then one `update_priority` write. Reaches no LLM, harness or process",
        ),
    },
    CommandOverride {
        command: "assess_all_priorities",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: same scoring, looped; both fallible calls inside the loop use `?`, \
             so a mid-loop failure returns `Err` rather than a short list as success",
        ),
    },
    CommandOverride {
        command: "remove_proposal_dependency",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: one `proposal_dependencies` delete, error propagated",
        ),
    },
    CommandOverride {
        command: "update_ideation_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: one UPDATE plus a read-back inside a single `db.run`. Declared \
             arms-auto-plan-verification: `auto_verify_draft_plans` written here is the gate \
             `plan_verification_service` reads before launching the verification agent, and no \
             detector models it",
        ),
    },
    CommandOverride {
        command: "update_ideation_effort_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: read-merge-upsert, every step `?`; effort changes HOW a spawned \
             agent runs, not WHETHER a scheduler launches one",
        ),
    },
    CommandOverride {
        command: "update_ideation_model_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: validated then one upsert; both project/global branches `?`",
        ),
    },
    CommandOverride {
        command: "update_agent_lane_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: one lane upsert. Declared arms-agent-spawn-harness: \
             `resolve_agent_spawn_settings` reads this row on the live spawn path to pick the \
             harness, model and effort an agent is actually launched with, and no detector \
             models it",
        ),
    },
    CommandOverride {
        command: "create_workflow",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: builds the workflow from input and creates it; every step \
             propagates. Touches no task and no transition service",
        ),
    },
    CommandOverride {
        command: "update_workflow",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: get-or-404 then one update; propagates. Replacing the column set \
             can leave a task's `internal_status` unmapped by any column, but that is a board \
             projection, not a task write — the module reaches neither `task_repo` nor a \
             transition service",
        ),
    },
    CommandOverride {
        command: "set_default_workflow",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: clear-then-set default. NOT a fail-open — the error is propagated, \
             not swallowed — but the pair runs under `db.run` (no BEGIN), so a failed second \
             write leaves ZERO defaults. Recorded as a product bug, not a refusal: the local UI \
             reaches the identical path, so refusing it would not fix it",
        ),
    },
    CommandOverride {
        command: "activate_methodology",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: deactivate-previous then activate; every await `?`. Same \
             non-atomic-toggle product bug as `set_default_workflow`, same reasoning",
        ),
    },
    CommandOverride {
        command: "deactivate_methodology",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-11 audit: single-row deactivate with a not-active guard; no partial window",
        ),
    },
    //
    // The twelve B4 detector-(c) refusals. Every one was hand-traced to a concrete
    // `Command::new` rather than accepted on the probe's boolean, because the scanner is known to
    // over-attribute: it treats `resolve_manual_role_spawn_settings` as launch-reaching when that
    // helper terminates in pure DB/YAML, and it confuses `resolve_node_cli_path` with the
    // `find_node_cli_path` that `git_cmd` reaches via `ensure_resolved_node_bin_in_path`. Between
    // them those two errors invented the identical {git, codex, node} triple on all five
    // `agent_plan_commands`. The reasons below claim only the sinks the trace CONFIRMED.
    process_refusal(
        "copy_agent_conversation_plan",
        "detector-c, hand-traced: seed_agent_conversation_plan prepares the plan workspace and \
         reaches GitService::get_current_branch, and on a conversation with no workspace yet also \
         create_worktree plus the project's pre-execution shell setup. The probe's codex/node \
         tokens are artifacts — this path never spawns an agent — but the git launch is real",
    ),
    process_refusal(
        "import_agent_conversation_plan",
        "detector-c, hand-traced: the same seed_agent_conversation_plan helper as \
         copy_agent_conversation_plan, genuinely shared, so the same git worktree launch. \
         codex/node artifacts likewise",
    ),
    process_refusal(
        "activate_agent_task_pipeline",
        "detector-c, hand-traced and NARROW: the command's own work is DB-only. The launch is \
         reached through agent_workspace_response_for_state's stale-publish repair, which runs \
         `git rev-parse --is-inside-work-tree` when the conversation has a stranded PR-fix review \
         handoff. Refused because the process-launch floor is absolute, not because the reach is \
         broad — recorded this way so a future seam split can be argued against the real path",
    ),
    process_refusal(
        "activate_agent_plan_direct_implementation",
        "detector-c, hand-traced and NARROW: same incidental publish-repair probe as \
         activate_agent_task_pipeline. This command flips the mode inline in SQL and does NOT \
         inherit the worktree-creation edge the copy/import pair carries",
    ),
    process_refusal(
        "start_agent_task_pipeline",
        "detector-c, hand-traced: delegates to apply_supervised_proposals_core and inherits all \
         three of apply_proposals_to_kanban's sinks — the repository capability probe, \
         base-branch creation, and the session-namer agent spawn",
    ),
    process_refusal(
        "create_ideation_session",
        "detector-c, hand-traced and UNCONDITIONAL: prepare_ideation_analysis_state calls \
         GitService::get_current_branch as the fourth statement of the impl, before any \
         branching, followed immediately by resolve_project_default_branch",
    ),
    process_refusal(
        "archive_ideation_session",
        "detector-c, hand-traced: TaskCleanupService::cleanup_tasks walks and deletes worktrees \
         and branches, then delete_feature_branch runs `git branch -D` for the session's active \
         plan branch",
    ),
    process_refusal(
        "reopen_ideation_session",
        "detector-c, hand-traced: SessionReopenService::reopen runs the same cleanup_tasks \
         worktree/branch walk and then delete_feature_branch. Reached through a different helper \
         from create/archive — all three are independently confirmed, none inherited",
    ),
    process_refusal(
        "spawn_session_namer",
        "detector-c, hand-traced: client.spawn_agent resolves the Codex CLI and the node binary \
         for MCP wiring, and the caller selects the harness through the provider_harness argument",
    ),
    process_refusal(
        "apply_proposals_to_kanban",
        "detector-c, hand-traced, three independent sinks: the github_pr_enabled capability probe \
         (ensure_git_worktree), base-branch creation directly controlled by the caller's \
         base_branch_override, and the session-namer agent spawn resolving codex and node",
    ),
    process_refusal(
        "restart_ideation_implementation",
        "detector-c, hand-traced: the capability probe, then GitService list/delete_worktree, \
         fetch_origin_branch_strict, and a reset_hard + clean_working_tree pair on the restarted \
         worktree — the destructive end of the range",
    ),
    process_refusal(
        "set_tasks_feature_enabled",
        "detector-c, hand-traced: toggling the feature ON fans out up to EIGHT Codex agent \
         spawns via reconcile_missing_assessments -> spawn_plan_complexity_assessor, gated only \
         on the caller's `enabled` argument and the prior Disabled state. Its read sibling \
         get_tasks_disable_impact is registered; the writer is foreclosed at every v1 scope",
    ),
    //
    // ---- PR 3.1-b batch 12 — census B5 (activity, automation, metrics, research) -----------
    //
    // Every row below replaces an `agent_default` placeholder with a reviewed reason. The block
    // is NOT shaped like B4's: `automation_commands` is the densest arming surface in the census
    // and contributes both the batch's whole floor and all four of its arming writes, while
    // `activity_commands` and `metrics_commands` are aggregate readers with no write at all.
    //
    // The reads. Dropped to `Read`/NONE on a body audit that found no repository write — never
    // on detector silence, which this batch has particular reason not to trust: detector (a)
    // fires on `save_metrics_config`, whose entire body is one `project_metrics_config` upsert,
    // because the bare name `execute` in `conn.execute(..)` resolves to
    // `AgentWorkflowRunner::execute` and drags 1200 nodes and a `send_message` sink in behind it.
    // See `probe_save_metrics_config_arming_evidence`.
    read_audit(
        "list_task_activity_events",
        "batch-12 audit: one cursor-paginated `activity_event_repo` read; the limit is clamped \
         to 100 host-side and every error is `map_err(..)?`",
    ),
    read_audit(
        "list_session_activity_events",
        "batch-12 audit: the same paginated read keyed by session; no write",
    ),
    read_audit(
        "list_all_activity_events",
        "batch-12 audit: unscoped paginated read. Widest reader in the block, but still a read — \
         the filter is the caller's own narrowing and omitting it is already the default",
    ),
    read_audit(
        "count_task_activity_events",
        "batch-12 audit: aggregate count; no write",
    ),
    read_audit(
        "count_session_activity_events",
        "batch-12 audit: aggregate count; no write",
    ),
    read_audit(
        "get_insights_stats",
        "batch-12 audit: cross-project aggregate query; the only `unwrap_or` defaults an absent \
         timezone/week-start ARGUMENT, never a swallowed Err",
    ),
    read_audit(
        "get_project_stats",
        "batch-12 audit: project-scoped twin of get_insights_stats, same shape",
    ),
    read_audit(
        "get_insights_pr_insights",
        "batch-12 audit: PR aggregate read; no write",
    ),
    read_audit(
        "get_project_pr_insights",
        "batch-12 audit: project-scoped twin of get_insights_pr_insights",
    ),
    read_audit(
        "get_insights_trends",
        "batch-12 audit: bucketed trend query; no write",
    ),
    read_audit(
        "get_project_trends",
        "batch-12 audit: project-scoped twin of get_insights_trends",
    ),
    read_audit(
        "get_metrics_config",
        "batch-12 audit: single `project_metrics_config` row read; 23-node closure, detectors \
         silent, and unlike its writing sibling it never touches the colliding `execute` name",
    ),
    read_audit(
        "get_task_metrics",
        "batch-12 audit: per-task metric read; no write",
    ),
    read_audit(
        "get_research_presets",
        "batch-12 audit: pure function over ResearchDepthPreset::all(); takes no AppState at all \
         and cannot read or write anything",
    ),
    read_audit(
        "get_research_process",
        "batch-12 audit: single `process_repo` read; no write",
    ),
    read_audit(
        "get_research_processes",
        "batch-12 audit: list read with an optional status filter that is `parse()?`-rejected on \
         a miss rather than defaulted, so a bad filter cannot silently widen the query",
    ),
    read_audit(
        "list_automations",
        "batch-12 audit: `AutomationService::list_automations`; the service is constructed from \
         AppState Arc clones, reaches no launch resolver, and this path performs no write",
    ),
    read_audit(
        "get_automation",
        "batch-12 audit: detail read plus `automation_detail_response_for_state`, whose usage, \
         pipeline and run hydrators all propagate with `?` — no `.ok()` anywhere on the path",
    ),
    //
    // The writers. Registered at `ui:agent`, never dropped to Read on detector silence.
    CommandOverride {
        command: "save_metrics_config",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: ONE `project_metrics_config` upsert inside a single `db.run`, error \
             propagated. Detector (a) fires on it and the hit is an attribution artifact, not a \
             finding: the bare name `execute` from `conn.execute(..)` resolves to \
             `AgentWorkflowRunner::execute`. Kept at AgentControl regardless — it is a write, and \
             a write is not dropped to Read on a detector verdict in either direction",
        ),
    },
    CommandOverride {
        command: "pause_research",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: guarded Running->Paused entity mutation then one `process_repo` \
             update; a wrong-status caller gets Err, not a silent no-op",
        ),
    },
    CommandOverride {
        command: "resume_research",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: guarded Paused->Running write. Canonicalized with the already \
             registered `start_research`, which reaches the SAME Running value: no production \
             consumer scans for Running ResearchProcess rows — the only reader is \
             startup_cleanup's fail_all_active — so this arms nothing and carries no \
             SeedsSpawnTriggeringState. If a research executor is ever wired, BOTH rows move",
        ),
    },
    CommandOverride {
        command: "stop_research",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: terminal-guarded write recording a user stop; authority-reducing",
        ),
    },
    CommandOverride {
        command: "pause_automation",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: one CAS status write to Paused. Authority-reducing — it removes the \
             Active value the automation scheduler scans for",
        ),
    },
    CommandOverride {
        command: "stop_automation",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: CAS write to Stopped. The Active write in this body is a ROLLBACK \
             restoring the pre-call value after a failed follow-up, not a fresh arming",
        ),
    },
    CommandOverride {
        command: "cancel_automation_run",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: ownership-checked run cancel via CAS. The trailing \
             sync_goal_items_for_closed_run_without_successor returns unit and absorbs its own \
             repo errors, but it is a derived goal-item projection running AFTER the cancel is \
             durable and returned — it cannot make a failed cancel look successful",
        ),
    },
    CommandOverride {
        command: "update_automation_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: one settings patch; plan_approval_mode and pr_merge_mode are both \
             `parse()`-validated and rejected on a miss. These knobs govern whether a LATER run \
             auto-approves a plan or auto-merges a PR, but they seed no scanned surface value on \
             their own — the run has to already exist and reach that gate",
        ),
    },
    CommandOverride {
        command: "update_automation_config",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: direct spawn-free automation setup patch using the same validated \
             settings-then-config service flow as the setup HTTP route. The twin requires an \
             exact expected_updated_at match before either write, so stale remote clients fail \
             closed without mutating the row; these fields only configure a later run and do \
             not arm the automation scheduler",
        ),
    },
    //
    // The four arming writes: each flips `automations.status` to Active, the armed value the
    // `automation-active` state surface names and `spawn_automation_scheduler` scans.
    CommandOverride {
        command: "restart_automation",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: CAS Stopped->Active. Declared arms-automation-scheduler: Active is \
             the armed value of the automation-active surface, but that surface's only write \
             marker is `reopen_run_corrective`, which this path does not carry, so detector (b) \
             is silent on a write that genuinely re-arms the scheduler",
        ),
    },
    CommandOverride {
        command: "resume_automation_run",
        policy: policy(
            RiskClass::AgentControl,
            SEEDS_STATE,
            "batch-12 audit: `reopen_automation_run` re-opens a closed run. Detector (a) AND (b) \
             both fire here — it carries the surface's `reopen_run_corrective` marker — so it is \
             the ONE arming member of this batch that earns SeedsSpawnTriggeringState, which \
             `seeds_spawn_triggering_state_tags_track_detector_b_evidence` defines as detector-(b) \
             evidence. Its three detector-silent siblings take AGENT plus a declared membership \
             instead, the same split batches 10 and 11 used",
        ),
    },
    CommandOverride {
        command: "retry_automation_plan_judge",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: does NOT spawn inline — unlike its `retry_automation_judge` twin it \
             never reaches dispatch_automation_run_now_action, which is why detector (c) is \
             correctly silent. It instead un-pauses the automation to Active and resets \
             plan_judge_state Failed->None on a run AwaitingPlanApproval, leaving exactly the \
             state the scheduler dispatches a fresh plan judge from. Declared \
             arms-automation-scheduler",
        ),
    },
    CommandOverride {
        command: "skip_automation_judge",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-12 audit: advances a run past its judge and, when the automation was paused \
             for a failed judge, flips Paused->Active. Skipping the judge is the point: it \
             removes the gate AND restores the scanned Active value. Declared \
             arms-automation-scheduler",
        ),
    },
    //
    // The batch-12 floor. Three members, each hand-traced to a concrete launch through the
    // reconstructed call path rather than accepted on detector (c)'s boolean.
    process_refusal(
        "create_automation_draft",
        "detector-c, hand-traced: create_automation_draft_for_state calls \
         prepare_agent_conversation_workspace_with_setup_mode_and_defaults, which reaches \
         GitService::ref_exists -> run_status -> build_git_command. Setup mode is Deferred, so \
         no worktree is materialised, but the ref probe is an unconditional git launch",
    ),
    process_refusal(
        "trigger_automation_run_now",
        "detector-c, hand-traced: dispatch_automation_run_now_action -> \
         spawn_automation_judge_task -> AutomationJudgeTask::invoke_and_parse_judge -> \
         invoke_automation_utility_agent -> CodexCliClient::spawn_agent, resolving the Codex CLI \
         and, through build_codex_internal_mcp_overrides, the node binary. A real agent spawn",
    ),
    process_refusal(
        "retry_automation_judge",
        "detector-c, hand-traced: the SAME dispatch_automation_run_now_action chain as \
         trigger_automation_run_now, genuinely shared rather than inferred, so the identical \
         Codex spawn. Its plan-judge sibling retry_automation_plan_judge does NOT share it and \
         is registered as an arming write instead",
    ),
    // ---------------------------------------------------------------------------------------
    // PR 3.1-b batch 13 — census `B7` (artifact, notification, release-notes, task-context, ui,
    // update-channel) plus two `B6` modules (persona, MCP policy). 52 members.
    //
    // Batch 12 swept B7 and recorded every member detector-SILENT except
    // `update_notification_settings`. This batch re-measured that sweep rather than inheriting it
    // and then read every body, because detector silence is exactly the condition under which a
    // batch is tempted to register without reading — and this block found THREE separate
    // attribution errors, one of them in the safety direction. See
    // `batch13_detector_gap_is_measured_not_inherited`.
    //
    // The reads first. Each is `Read` on a body audit that found no repository write; none was
    // bought by a silent detector.
    // ---------------------------------------------------------------------------------------
    read_row(
        "get_artifacts",
        "batch-13 audit: a single artifact_repo.get_by_type behind a parsed filter, error via \
         map_err. The `artifact_type: None` branch returns Ok(vec![]) rather than all artifacts — \
         recorded as a product bug, not a fail-open: it is an unimplemented filter path, not a \
         swallowed host error",
    ),
    read_row(
        "get_artifact",
        "batch-13 audit: one artifact_repo.get_by_id; Option is preserved into the response and \
         the repository error propagates through map_err",
    ),
    read_row(
        "get_artifact_at_version",
        "batch-13 audit: one artifact_repo.get_by_id_at_version, Option preserved, error mapped",
    ),
    read_row(
        "get_artifacts_by_bucket",
        "batch-13 audit: one artifact_repo.get_by_bucket, error mapped",
    ),
    read_row(
        "get_artifacts_by_task",
        "batch-13 audit: one artifact_repo.get_by_task, error mapped",
    ),
    read_row(
        "get_artifact_version_history",
        "batch-13 audit: one artifact_repo.get_version_history, error mapped",
    ),
    read_row(
        "get_buckets",
        "batch-13 audit: one artifact_bucket_repo.get_all, error mapped",
    ),
    read_row(
        "get_system_buckets",
        "batch-13 audit: takes NO AppState — a pure function over the compiled-in system bucket \
         table, in the `get_research_presets` shape batch 12 registered",
    ),
    read_row(
        "get_artifact_relations",
        "batch-13 audit: one artifact_repo.get_relations, error mapped",
    ),
    read_row(
        "get_task_context",
        "batch-13 audit: TaskContextService::get_task_context over five repositories; the error \
         match discriminates NotFound from other AppError variants and both propagate. No write",
    ),
    read_row(
        "get_artifact_full",
        "batch-13 audit: artifact_repo.get_by_id with the repository error mapped and the absent \
         row turned into an explicit NOT-FOUND message, never an empty success",
    ),
    read_row(
        "get_artifact_version",
        "batch-13 audit: artifact_repo.get_by_id_at_version, same explicit-absence shape",
    ),
    read_row(
        "get_related_artifacts",
        "batch-13 audit: one artifact_repo.get_related, error mapped",
    ),
    read_row(
        "search_artifacts",
        "batch-13 audit: fans out get_by_type over the requested types and filters in memory; the \
         type parse propagates with `?`, so an unknown type ERRORS. Its HTTP namesake in \
         http_server/handlers/worker.rs silently skips unparsable types — the Tauri command \
         audited here is the fail-closed half",
    ),
    read_row(
        "get_notification_settings",
        "batch-13 audit: one notification_settings_repo.get_settings, error mapped",
    ),
    read_row(
        "get_unread_notification_count",
        "batch-13 audit: one notification_repo.unread_count, error mapped",
    ),
    read_row(
        "list_attention_items",
        "batch-13 audit: AttentionService::list_attention_items, documented and confirmed \
         fail-closed — an unloadable authoritative source errors rather than shipping a partial \
         list as complete",
    ),
    read_row(
        "list_notifications",
        "batch-13 audit: one cursor-paginated notification_repo.list; the `limit.unwrap_or(50)` \
         defaults an absent ARGUMENT, never a swallowed Err",
    ),
    read_row(
        "get_current_release_notes",
        "batch-13 audit: reads the packaged version and resolves the notes file through \
         sanitize_release_notes_version, which rejects `..`, separators and non-ASCII. An \
         unreadable candidate yields source: Missing — an EXPLICIT tri-state the caller can see, \
         not a fabricated body",
    ),
    read_row(
        "get_release_notes_for_version",
        "batch-13 audit: same path as get_current_release_notes with a caller-supplied version, \
         through the same containment check",
    ),
    read_row(
        "get_last_seen_release_notes_version",
        "batch-13 audit: one app_state_repo.get projecting a single field, error mapped",
    ),
    read_row(
        "list_release_notes_versions",
        "batch-13 audit: FIXED THEN REGISTERED. The reader was `std::fs::read_dir(path).ok()`, so \
         a permissions or I/O failure produced an empty version list indistinguishable from a \
         genuine empty directory. collect_versions_from_dirs now returns io::Result: an ABSENT \
         root is still skipped (one of the two candidates is always absent by construction) while \
         any other error propagates",
    ),
    read_row(
        "list_personas",
        "batch-13 audit: PersonaService::list_personas behind the feature flag, error mapped. 29 \
         closure nodes",
    ),
    read_row(
        "get_persona",
        "batch-13 audit: PersonaService::get_persona, error mapped",
    ),
    read_row(
        "list_persona_usage",
        "batch-13 audit: PersonaService::list_persona_usage, error mapped",
    ),
    read_row(
        "preview_persona_overlay",
        "batch-13 audit: resolves the overlay a conversation WOULD receive and returns the \
         rendered block on the direct command response only; the empty-id guard errors first and \
         the chat-service error propagates. Renders, persists nothing",
    ),
    read_row(
        "get_ui_feature_flags",
        "batch-13 audit: projects the OnceLock runtime config plus the agent-capability snapshot. \
         Infallible and write-free",
    ),
    read_row(
        "get_update_channel",
        "batch-13 audit: one app_state_repo.get projecting update_channel, error mapped. Its \
         WRITE half `set_update_channel` is deliberately NOT registered — see its row",
    ),
    //
    // The writers. Registered at `ui:agent` on a body audit; a silent detector never licensed
    // dropping one to Read, and a firing detector never by itself bought a refusal.
    CommandOverride {
        command: "archive_artifact",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: artifact_repo.archive sets archived_at and the repository error \
             propagates with `?`; the follow-up app.emit is `.ok()`-discarded but runs AFTER the \
             write is durable and cannot make a failed archive look successful. Carries \
             MutatesAgentConsumedContent for the same reason create_artifact/update_artifact do — \
             hiding an artifact changes what agents subsequently read",
        ),
    },
    CommandOverride {
        command: "create_bucket",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-13 audit: builds an ArtifactBucket from validated accepted-types (an unknown \
             type ERRORS) plus writer/reader lists, then one artifact_bucket_repo.create. Creates \
             a CONTAINER, not agent-consumed content, so it does not take MutatesAgentConsumedContent",
        ),
    },
    CommandOverride {
        command: "mark_notification_read",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-13 audit: notification_service().mark_read, error propagated with `?`",
        ),
    },
    CommandOverride {
        command: "mark_all_notifications_read",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-13 audit: notification_service().mark_all_read over an optional project scope, \
             error propagated with `?`",
        ),
    },
    CommandOverride {
        command: "set_dock_badge_count",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-13 audit: mirrors a frontend-owned count onto the macOS Dock tile through \
             run_on_main_thread, whose error propagates. Persists nothing and reads no domain \
             state; classified AgentControl because it is a host-visible side effect, and NOT \
             HostManagement — every HOST row in this ledger sits at a class v1 does not grant, \
             and a cosmetic badge is not that authority",
        ),
    },
    CommandOverride {
        command: "update_notification_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-13 audit: applies only the Some(..) fields to NotificationSettings and writes \
             one update_settings; both repository calls propagate. Detector (b) FIRES on it and \
             the row deliberately does NOT claim SeedsSpawnTriggeringState: the flag is a MARKER \
             collision, measured as entry=workspace-auto-review with \
             write_markers_matched=[\"update_settings\"] and armed_matched=[\"require_workspace_review\"]. \
             The notification settings repository method merely SHARES a bare name with the \
             workspace-review write marker. Claiming the tag would have PASSED \
             seeds_spawn_triggering_state_tags_track_detector_b_evidence, which only enforces \
             tag -> evidence, while being false",
        ),
    },
    CommandOverride {
        command: "mark_release_notes_seen",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-13 audit: sanitizes the version through the same containment check the read \
             half uses, then one app_state_repo.set_last_seen_release_notes_version",
        ),
    },
    //
    // The eight persona writes. All eight are `AgentControl`; SEVEN of them are reported
    // `a=true` by detector (a) and that verdict is an ARTIFACT — see
    // `batch13_detector_gap_is_measured_not_inherited`. The class comes from the bodies, which
    // are writes, so the collision changed no disposition; it is recorded so the reasons are true.
    CommandOverride {
        command: "create_persona_draft",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: composes persona content (content, or description+body, else ERROR) \
             and writes a draft through PersonaService::create_draft with map_err(to_string)?; \
             the follow-up emit_draft_updated runs after the write. Persona bodies are injected \
             into agent prompts, hence MutatesAgentConsumedContent",
        ),
    },
    CommandOverride {
        command: "update_persona_draft",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: PersonaService::update_draft carrying an optional \
             expected_content_hash — an OPTIMISTIC-CONCURRENCY check, propagated not swallowed",
        ),
    },
    CommandOverride {
        command: "update_persona",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: re-reads the existing persona for its slug, recomposes content, then \
             PersonaService::update_persona; every hop map_err(to_string)?",
        ),
    },
    CommandOverride {
        command: "approve_persona",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: reads the draft's source_persona_id, approves through \
             PersonaService::approve_persona, and emits persona:draft_applied only when the \
             approved id MATCHES the recorded source. Promotes a draft to the content live \
             conversations overlay",
        ),
    },
    CommandOverride {
        command: "approve_persona_as_new",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: PersonaService::approve_persona_as_new with an optional new slug, \
             error mapped. Forks rather than overwrites; same content authority",
        ),
    },
    CommandOverride {
        command: "reseed_persona_draft",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: PersonaService::reseed_persona_draft resets a draft to its source, \
             error mapped",
        ),
    },
    CommandOverride {
        command: "archive_persona",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: PersonaService::archive_persona, error mapped. Withdraws a persona \
             from overlay resolution",
        ),
    },
    CommandOverride {
        command: "unarchive_persona",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "batch-13 audit: PersonaService::unarchive_persona, error mapped. The measured \
             CONTRAST that proves the sibling collision is an artifact: identical shape to \
             archive_persona but 120 closure nodes and a=FALSE, exactly the get_metrics_config \
             (23) vs save_metrics_config (1200) asymmetry batch 12 pinned",
        ),
    },
    //
    // MCP policy. Seven refusals.
    // The four MCP override writes were registered at AgentControl on the
    // update_agent_lane_settings precedent (batch-13 audit: three fail-closed guards —
    // ensure_project_scope_exists, ensure_mutation_ready, mutable_key — then one audited-clean
    // McpPolicyService write). The #976 probe cache voided that disposition: the eligibility
    // guard's probe fallback now resolves CLI paths, so every member reaches the absolute
    // floor. The write bodies are still clean; a spawn-free intent twin can restore remote
    // parity without touching the local seam.
    process_refusal(
        "update_mcp_server_override",
        "detector-c: ensure_mutation_ready -> resolve_provider_management_eligibility -> \
         refresh_harness_runtime_probe -> cached_harness_runtime_refresh_probe -> \
         resolved_harness_binary_path -> find_claude_cli. The eligibility guard that runs \
         BEFORE the write resolves the harness binary path even on the cache-hit branch, so \
         the floor forecloses the row at every v1 scope. Declared \
         configures-future-agent-tool-authority — the membership states what the command DOES \
         and survives the class correction",
    ),
    process_refusal(
        "clear_mcp_server_override",
        "detector-c: the SAME ensure_mutation_ready -> resolve_provider_management_eligibility \
         -> refresh_harness_runtime_probe chain as update_mcp_server_override. Declared \
         configures-future-agent-tool-authority, as its update half is",
    ),
    process_refusal(
        "update_mcp_tool_override",
        "detector-c: the SAME ensure_mutation_ready -> resolve_provider_management_eligibility \
         -> refresh_harness_runtime_probe chain as update_mcp_server_override. Declared \
         configures-future-agent-tool-authority, as its server-scoped sibling is",
    ),
    process_refusal(
        "clear_mcp_tool_override",
        "detector-c: the SAME ensure_mutation_ready -> resolve_provider_management_eligibility \
         -> refresh_harness_runtime_probe chain as update_mcp_server_override. Declared \
         configures-future-agent-tool-authority",
    ),
    CommandOverride {
        command: "update_ui_feature_flags",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-13 audit: persists the agent-personas override and the team/workflows/autopilot \
             capability gates, each repository error propagated, then republishes the snapshot. \
             Declared configures-future-agent-capability-gates: the \
             personas override changes injected prompt content and the capability gates change \
             which agent modes exist. Registerable at AgentControl for the same reason the MCP \
             override rows are — see update_mcp_server_override on why the literal \
             ConfiguresFutureProcessAuthority reading is unrepresentable here. Recorded product bug: the \
             two writes are not atomic — a failed capability write leaves the personas override \
             already applied to both the repository and the process-global",
        ),
    },
    //
    // The batch-13 floor. THREE members, and the third is the first hand-traced launch in this
    // series that detector (c) does NOT see.
    process_refusal(
        "get_mcp_catalog",
        "detector-c: build_catalog -> discover_provider_catalog -> resolve_codex_catalog_cli_path \
         -> resolve_codex_cli, and the Codex branch then runs \
         discover_native_mcp_servers_via_app_server against that CLI path. A READ by intent that \
         launches the Codex app-server to answer; the floor is absolute and does not care which. \
         Remote readers use the spawn-free get_remote_mcp_catalog snapshot twin",
    ),
    process_refusal(
        "refresh_mcp_catalog",
        "detector-c: the SAME build_catalog chain as get_mcp_catalog, reached with an explicit \
         provider rather than an optional one. Remote readers use the spawn-free \
         get_remote_mcp_catalog snapshot twin; refresh remains host-local",
    ),
    process_refusal(
        "retry_legacy_mcp_registration_repair",
        "detector-c: ensure_mutation_ready -> resolve_provider_management_eligibility -> \
         refresh_harness_runtime_probe -> resolved_harness_binary_path -> find_claude_cli — \
         the #976 probe-cache path made this command detector-visible, so the row moved from \
         the hand-traced miss to a measured refusal. The ORIGINAL launch it was refused for is \
         still invisible: it runs `claude mcp remove ralphx -s user` through \
         tokio::process::Command::new at \
         infrastructure/agents/claude/mcp_registration_repair.rs, via \
         retry_reserved_claude_registration_repair -> reconcile_reserved_claude_registration -> \
         {resolve_claude_cleanup_cli, remove_reserved_user_registration}, and two mechanisms \
         still hide that path: resolve_claude_cleanup_cli reaches find_claude_cli only by \
         passing it to spawn_blocking as a bare function VALUE, which creates no call edge, and \
         remove_reserved_user_registration spawns an already-resolved path, naming no resolver \
         for the sink model to match. batch13_detector_gap_is_measured_not_inherited pins both \
         mechanisms as still-open root-level gaps. Remote MCP settings use the spawn-free \
         get_remote_mcp_catalog snapshot twin and leave repair unavailable",
    ),
    // Same #976 root cause as the MCP override block, reached through the availability helper
    // instead of the mutation guard. Their batch-11 FailOpenUntilFixed findings (the
    // .ok().flatten() answer-changing fail-open in get_harness_availability_for_lanes) are
    // still real, but the mechanical floor supersedes the audit rows — batch 9 requires audit
    // rows only where the mechanical resolution would otherwise be Registerable.
    process_refusal(
        "get_agent_harness_availability",
        "detector-c: get_harness_availability_for_lanes -> \
         refreshed_provider_aware_runtime_probes -> refresh_supported_harnesses -> \
         refresh_harness_runtime_probe_with_force -> cached_harness_runtime_refresh_probe -> \
         resolved_harness_binary_path -> find_claude_cli. The #976 probe cache keys rows by \
         the resolved binary path, so even the cache-hit branch resolves CLI paths; a READ by \
         intent that reaches the launch floor, and the floor does not care which",
    ),
    process_refusal(
        "get_ideation_harness_availability",
        "detector-c: the SAME get_harness_availability_for_lanes chain as \
         get_agent_harness_availability, over IDEATION_LANES instead of AGENT_LANES — one \
         shared helper, two ledger rows, so re-auditing the helper clears both",
    ),
    CommandOverride {
        command: "set_update_channel",
        policy: policy(
            RiskClass::Elevated,
            HOST,
            "batch-13 audit: one app_state_repo.set_update_channel with the error propagated — \
             the BODY is clean, and the refusal is about authority, not hygiene. It selects which \
             release train the desktop app auto-updates onto, which is host management, and every \
             other HOST row in this ledger (remote_device, remote_environment, remote_host, \
             startup) sits at Elevated for the same reason. V1Deferred, NOT denied: a later scope \
             may grant it. Its READ half `get_update_channel` is registered",
        ),
    },

    // ---------------------------------------------------------------------------------------
    // PR 3.1-b batch 14 — THE FINAL BATCH. The last 48 ratchet members, driving P-11 to ZERO.
    //
    // The batch's central measurement: detector (c) was SILENT on 36 of the 48, and the hand
    // trace found a real, unconditional process launch behind THIRTEEN of them. Batch 13 pinned
    // two hiding mechanisms; this batch confirms the second is far larger than one victim and
    // adds a third.
    //
    //   M1  `spawn_blocking(bare_fn)` / a function VALUE stored in a struct field creates no
    //       call edge. New site found here: `running_agent_registry.rs:45` stores
    //       `kill: Arc::new(kill_process)` and `stop_if_owned` calls it as `(self.kill)(pid)`,
    //       so nothing syntactically names `kill_process`, which runs
    //       `Command::new(resolve_pkill_cli_path())` at `:323`.
    //   M2  `Command::new(<already-resolved path variable>)` names no resolver, and the sink
    //       model is resolver-NAME-based. This is not a corner case: it is how EVERY agent
    //       launch in the codebase is written — `claude_code_client.rs:596/:681`,
    //       `codex/mod.rs:1156/:1206/:1286`, `git_cmd.rs:272`, `agent_workflow_runner.rs:162`.
    //   M3  dyn-trait dispatch (`Arc<dyn ChatService>`, `services.agent_spawner`,
    //       `TransitionHandler::on_enter`'s state-arm dispatch) breaks the edge before the
    //       resolver is ever named.
    //
    // The engine is deliberately NOT widened, the same call batch 13 made and for a stronger
    // reason now that the blast radius is known: widening it would retroactively move rows
    // batches 7-13 dispositioned, and the ratchet reaches zero in THIS batch, so there is no
    // successor batch to absorb the churn. Every refusal below that detector (c) cannot see is
    // marked `detector-c-MISS, hand-traced` and pinned by a test that asserts detector (c) is
    // still silent, so closing the gap later fails loudly instead of drifting into agreement.
    // ---------------------------------------------------------------------------------------

    // --- Floor, detector (c) DETECTED. Reaching a CLI resolver forecloses v1 regardless of
    //     intent; these need no hand argument beyond the measured path.
    process_refusal(
        "resume_task",
        "detector-c: publish_post_merge_branch_update -> run_authorized_mutation -> \
         build_git_command; request_remote_task_resume is the spawn-free intent twin and \
         spawn_remote_resume_dispatchers alone calls this host seam",
    ),
    process_refusal(
        "retry_branch_update",
        "detector-c: execute_programmatic_branch_update -> run_authorized_mutation -> \
         build_git_command; hand-tracing adds two more independent launches (the post-merge \
         publish push, and entry actions into UpdatingTaskBranch/UpdatingPlanBranch which \
         start the branch-update resolver agent)",
    ),
    process_refusal(
        "restart_task",
        "detector-c: validate_resume -> GitService::branch_exists -> run_status -> \
         build_git_command; request_remote_task_restart is the spawn-free intent twin and \
         spawn_remote_resume_dispatchers alone calls this host seam",
    ),
    process_refusal(
        "recover_task_execution",
        "detector-c: recover_execution_stop -> apply_recovery_decision -> \
         reconcile_merge_auto_complete -> try_complete_stale_rebase -> build_git_command, and \
         separately is_ipr_process_alive -> is_process_alive",
    ),
    process_refusal(
        "resolve_recovery_prompt",
        "detector-c: apply_user_recovery_action -> apply_failed_user_recovery_action -> \
         GitService::delete_branch -> build_git_command",
    ),
    process_refusal(
        "switch_agent_conversation_mode",
        "detector-c: the running-agent-stopping mode switch prepares the conversation \
         workspace (GitService::ref_exists) and reaches the publish path's \
         inspect_repository_capability -> ensure_git_worktree",
    ),
    process_refusal(
        "set_agent_conversation_workspace_pr_supervision",
        "detector-c: the same resolve_agent_workspace_pr_automation_target worktree path as \
         set_agent_conversation_workspace_auto_publish, genuinely shared",
    ),
    process_refusal(
        "reconcile_agent_conversation_workspace_publication",
        "detector-c: schedule_pr_supervision_recovery_for_conversation_id -> \
         recover_agent_workspace_pr_supervision -> GitService::get_head_sha",
    ),
    process_refusal(
        "commit_agent_conversation_workspace_locally",
        "detector-c: commit_agent_workspace_locally_unlocked -> GitService::get_head_sha; the \
         command's whole purpose is a local git commit",
    ),
    process_refusal(
        "precompute_agent_conversation_workspace_pr_description",
        "detector-c, two sinks: draft_agent_workspace_pr_metadata_decision_unlocked runs \
         run_git_text for the diff AND reaches CodexCliClient::spawn_agent -> \
         resolve_codex_cli to draft the description with an agent",
    ),
    process_refusal(
        "archive_agent_conversation",
        "detector-c: archive_agent_conversation_for_state -> \
         terminalize_agent_workspace_after_pr -> cleanup_force_owned_terminal_artifacts -> \
         GitService::branch_exists; archiving walks and deletes worktrees and branches",
    ),

    // `resume_execution` became detector-(c) visible after Wave B1 extracted the shared
    // state-only seam and wired the host dispatcher to that same call graph.
    // --- Floor, detector (c) MISSED. Each was reported c=false and each launches anyway. The
    //     hiding mechanism is named per row so a successor can close it deliberately.
    process_refusal(
        "resume_execution",
        "detector-c after the Wave B1 shared-seam extraction; THREE independent launch chains. (1) \
         execute_entry_actions -> TransitionHandler::on_enter -> enter_executing_state -> \
         send_task_execution_message; (2) try_schedule_ready_tasks transitions a Ready task to \
         Executing into the same spine; (3) four paused-queue relaunchers reach \
         ChatService::send_message. All terminate at ChatHarnessLaunchPlan::spawn -> \
         Command::new(cli_path) (claude/mod.rs:664, codex/mod.rs:1156), which names no \
         resolver. It ALSO fails open at lifecycle.rs:273: the task is transitioned into an \
         agent-active status at :258, then `if let Ok(Some(..))` collapses read error and \
         absence, so entry actions never run, restoring_count is never incremented, and the \
         capacity guard admits MORE tasks than the cap while the command returns success; \
         request_remote_execution_resume is the spawn-free intent twin and the host dispatcher \
         alone calls this seam",
    ),
    process_refusal(
        "update_execution_settings",
        "detector-c-MISS (M2+M3), hand-traced: settings.rs:147 tokio::spawn -> \
         PendingSessionDrainService::try_drain_pending_for_project -> send_message -> agent \
         spawn, plus the scheduler kick at :113. It also fails open at settings.rs:88, where \
         `.map(..).unwrap_or(input.project_ideation_max)` on a Result makes a failed read look \
         like `value unchanged`, so a capacity raise is silently dropped",
    ),
    process_refusal(
        "update_global_execution_settings",
        "detector-c-MISS (M2+M3), hand-traced: resume_paused_workspace_queues_with_chat_service \
         reaches send_message, and :353 kicks the ready-task scheduler",
    ),
    process_refusal(
        "reset_agent_conversation_role_default",
        "detector-c-MISS (M1+M2+M3), hand-traced: service.stop_agent -> \
         running_agent_registry stop -> stop_if_owned -> (self.kill)(pid) -> kill_process -> \
         Command::new(resolve_pkill_cli_path()). The kill is reached through a function VALUE \
         held in a struct field, which is M1 in a shape batch 13 had not seen. It also \
         constructs a full AppChatService unconditionally before knowing whether any agent is \
         live",
    ),
    process_refusal(
        "resume_tasks_in_group",
        "detector-c-MISS (M2+M3), hand-traced: mutation.rs:2143/:2161 transition each task back \
         to its PRE-PAUSE status and run execute_entry_actions for it, so the restored status is \
         Executing/Reviewing/Merging and the on_enter spine spawns the agent; \
         request_remote_group_resume is the spawn-free intent twin and the host dispatcher \
         alone calls this seam",
    ),
    process_refusal(
        "pause_execution_plan",
        "detector-c-MISS (M1+M2+M3), hand-traced: only AGENT_ACTIVE tasks are touched, so \
         on_exit from Executing ALWAYS runs auto_commit_on_execution_done (git \
         has_uncommitted_changes + commit_all), and stop_task_runtime_contexts reaches \
         kill_process -> Command::new(resolve_pkill_cli_path())",
    ),
    process_refusal(
        "resume_execution_plan",
        "detector-c-MISS (M2+M3), hand-traced, three ways: transition_task into a gated \
         AGENT_ACTIVE restore status, execute_entry_actions, and an explicit \
         scheduler.try_schedule_ready_tasks()",
    ),
    process_refusal(
        "stop_execution_plan",
        "detector-c-MISS (M1+M2+M3), hand-traced: the same on_exit auto-commit and \
         stop_task_runtime_contexts kill path as pause_execution_plan, via \
         transition_to_stopped_with_context",
    ),
    process_refusal(
        "fork_agent_conversation",
        "detector-c-MISS (M2), hand-traced: prepare_agent_conversation_workspace_* runs \
         GitService get_current_branch/ensure_local_branch_from_origin_if_missing/ \
         get_branch_sha/create_worktree, then backgrounds run_pre_execution_setup, which \
         executes the project's setup commands through a shell AFTER the command has returned",
    ),
    process_refusal(
        "switch_agent_conversation_persona",
        "detector-c-MISS (M1+M2+M3), hand-traced: constructs the chat service and calls \
         stop_agent, reaching kill_process -> Command::new(resolve_pkill_cli_path()) plus a raw \
         SIGTERM that names no binary at all; switch_remote_agent_conversation_persona is the \
         spawn-free twin",
    ),
    process_refusal(
        "send_queued_agent_message_now",
        "detector-c-MISS (M2+M3), hand-traced, and the highest-authority command in the batch: \
         send_queued_message_with_policy(ManualNow) first stop_agent's the in-flight provider \
         (pkill + SIGTERM) and then send_message's a fresh turn, i.e. launch_plan.spawn(). It \
         does not re-timestamp a row; it interrupts one agent process and starts another",
    ),
    process_refusal(
        "stop_agent",
        "detector-c-MISS (M1+M2), hand-traced: AppChatService::stop_agent drops the \
         InteractiveProcess (EOF to the child's stdin), then running_agent_registry stop -> \
         kill_process -> Command::new(resolve_pkill_cli_path()).args([\"-TERM\", \"-P\", pid]) \
         plus nix SIGTERM. It terminates the agent child AND its whole child tree including the \
         MCP node servers — not in-memory state",
    ),
    process_refusal(
        "update_agent_conversation_coordination_mode",
        "detector-c-MISS (M2), hand-traced and CONDITIONAL: selecting CodexNativeUltra reaches \
         codex_ultra_support_for_model -> probe_harness -> probe_codex_cli, which runs up to \
         six `codex` subprocesses (--version, --help, exec --help, features list, debug models) \
         per candidate path on the first uncached call. Solo/Team/Workflow never reach it. \
         Conditional launches still foreclose: the caller picks the mode",
    ),

    // --- Deferred by AUTHORITY, not by hygiene: Elevated + ConfiguresFutureProcessAuthority,
    //     which class_permits admits only under `ui:elevated`. V1Deferred, NOT denied.
    //
    //     This is a DELIBERATE departure from batch 13's idiom and the line is drawn narrowly.
    //     Batch 13 refused the literal `ConfiguresFutureProcessAuthority` reading because it
    //     converted audited-clean BOUNDED writes (an MCP override, a lane's model/effort) into
    //     deferrals by notation. These three are not bounded: two write the spawned agent's
    //     SECURITY ENVELOPE (`sandbox_mode`, `approval_policy`) and one widens a later agent's
    //     filesystem reach to an arbitrary host directory. Where the deferred authority IS the
    //     containment boundary, the capability is the honest row. The batch-13 idiom is kept
    //     for this batch's model/effort writes, which stay registerable — see
    //     `update_workspace_review_runtime_settings` and `upsert_custom_agent_model`.
    CommandOverride {
        command: "add_conversation_folder_reference",
        policy: policy(
            RiskClass::Elevated,
            FUTURE_PROCESS,
            "the stored folder_path is read at spawn time by \
             resolve_mcp_filesystem_read_roots_with_folder_references and appended to the MCP \
             filesystem roots enforced for every subsequently spawned agent conversation. \
             Containment is real but stops short: validate_registration_path rejects relative, \
             ParentDir, root, symlink, non-directory and app-data paths, and re-validates on \
             read — but there is NO project-root or allowlist confinement, so any absolute \
             non-root directory (~/.ssh, /etc, another user's project) is accepted. A remote \
             caller could hand a later agent read access to arbitrary host directories. \
             Deferred, not denied: an allowlist confinement unlocks it",
        ),
    },
    CommandOverride {
        command: "update_manual_role_default",
        policy: policy(
            RiskClass::Elevated,
            FUTURE_PROCESS,
            "writes the whole tuple a later spawn consumes through \
             resolve_manual_role_spawn_settings -> ResolvedAgentSpawnSettings: harness, model, \
             effort, service_tier, coordination_mode, persona_id, and critically \
             approval_policy and sandbox_mode. The last two are the spawned agent's security \
             envelope rather than a preference, which is what separates this from the bounded \
             lane/MCP writes batch 13 registered with a declared membership",
        ),
    },
    CommandOverride {
        command: "clear_manual_role_default",
        policy: policy(
            RiskClass::Elevated,
            FUTURE_PROCESS,
            "the destructive half of update_manual_role_default and the more dangerous one: it \
             performs NO validation at all, deleting the row so resolution falls through to \
             project YAML, global, legacy lane, then provider default. A hardened \
             approval_policy/sandbox_mode can therefore be silently downgraded to whatever the \
             fallback layer says, with nothing in the response indicating the demotion",
        ),
    },

    // --- Registered `ui:read`: pure reads, errors propagated, audited clean.
    read_row(
        "get_start_composer_role_default",
        "batch-14 audit: resolves the backend-owned role default for a NEW conversation and \
         returns it. No write, no launch; every repository error propagates. Deliberately NOT \
         sharing get_manual_role_defaults' catalog_entry fallback, which is why that sibling is \
         refused and this one is registered",
    ),
    read_row(
        "get_agent_conversation_role_default",
        "batch-14 audit: the same resolve_composer_role_default read as \
         get_start_composer_role_default, keyed by an existing conversation. Same clean \
         error handling",
    ),
    read_row(
        "get_workspace_review_runtime_settings",
        "batch-14 audit: two fetch_many branches, both propagating with `?`. An empty vec \
         genuinely means `no rows`, never `the read failed`. Recorded semantic caveat, not a \
         fail-open: project_id=None returns GLOBAL rows only and does not merge project scope; \
         effective resolution is a separate concern owned by resolve_effective",
    ),

    // --- Registered `ui:agent`: writers, none dropped to Read on a silent detector.
    CommandOverride {
        command: "archive_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-14 audit: writes tasks.archived_at and updated_at through a pure db.run SQL \
             path behind authorize_task_mutation. No transition service, no entry/exit actions, \
             no scheduler, no registry, no git — hand-traced, not inferred from silence. \
             DISARMS scheduling (get_oldest_ready_tasks filters archived_at IS NULL). Recorded \
             standing wart, unchanged by this batch: archiving does not kill an already-running \
             agent for the task, so an Executing task can go invisible to the reconciler",
        ),
    },
    CommandOverride {
        command: "restore_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-14 audit: the exact inverse of archive_task and the same pure-SQL body. It \
             is a real ARM despite launching nothing: clearing archived_at on a task already in \
             Ready re-admits it to get_oldest_ready_tasks, so the next scheduler tick may spawn \
             for it. It does NOT claim SeedsSpawnTriggeringState — detector (b) does not flag \
             it, and that capability is defined as detector-(b) evidence, so the tag would be \
             false even though the arming is real",
        ),
    },
    CommandOverride {
        command: "create_agent_conversation",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-14 audit: inserts the conversation row and, for standalone/persona-builder \
             modes, creates a private workspace directory through standalone_workspace, whose \
             fs calls are containment-checked. Hand-traced clear of all four workspace helper \
             families that make its siblings spawn — it never reaches \
             prepare_agent_conversation_workspace_*, so no worktree and no setup shell",
        ),
    },
    CommandOverride {
        command: "restore_agent_conversation",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-14 audit: one restore() un-archiving the conversation row, errors \
             propagated, no launch and no fail-open. The narrowest write in the batch",
        ),
    },
    CommandOverride {
        command: "update_agent_conversation_title",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-14 audit: writes the conversation title and the linked ideation-session \
             title. Recorded and deliberately NOT treated as a blocking fail-open: \
             unified_chat_commands/mod.rs:11652 uses `.ok()?` on the message read used to \
             normalise a Jira key, so a repo error degrades to `no key found`. The command \
             still returns the title it actually stored, so no caller is told a write \
             succeeded that did not — it loses a cosmetic normalisation, not an authority answer",
        ),
    },
    CommandOverride {
        command: "update_workspace_review_runtime_settings",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "content-surface, declared membership configures-future-agent-runtime: upserts \
             model and effort for the Workspace Review background agent, read back by \
             resolve_explicit_workspace_review_runtime_settings. This is batch 13's \
             update_agent_lane_settings idiom exactly — BOUNDED deferred authority over which \
             model runs, not over the sandbox/approval envelope — so it stays registerable with \
             the finding declared rather than becoming a deferral by notation. Fail-closed: a \
             missing post-upsert re-read is an error, not a default",
        ),
    },
    CommandOverride {
        command: "upsert_custom_agent_model",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "content-surface, declared membership configures-future-agent-runtime: writes an \
             agent_model_registry row consumed by normalize_agent_runtime_selection to pick the \
             harness CLI's model and effort. Same bounded-deferred-authority idiom as \
             update_workspace_review_runtime_settings. Registered only AFTER fixing the \
             fail-open this batch found in its return path: the repo read `.ok()`-swallowed the \
             created_at/updated_at columns and fell through to a fabricated Utc::now(), so a \
             column error was rendered as a real timestamp",
        ),
    },
    CommandOverride {
        command: "approve_fix_task",
        policy: policy(
            RiskClass::AgentControl,
            AGENT,
            "batch-14: the STANDING HELD half, released. Batch 10 audited the body clean (a \
             Blocked guard, then Blocked->Ready in the registered unblock_task shape, every \
             error propagated) and withheld it ONLY on a pair argument — no remote way to \
             reject. That argument does not survive the current registry: block_task and \
             stop_task are both registered, so the remote brake exists; its exact \
             scheduler-construction shape is already registered as approve_task_for_review; and \
             the in-band Ready->Executing spawn is MODELLED rather than hidden, because \
             TRANSITION_SINKS cuts traversal at transition_task and SCHEDULER_SINKS names \
             try_schedule_ready_tasks, so the hit is classified by its target. Pinned corrective- \
            free: unlike its partner it reaches no corrective sink",
        ),
    },
    CommandOverride {
        command: "log_frontend_error",
        policy: policy(
            RiskClass::Elevated,
            HOST,
            "host-only log sink: each call truncates its three fields before tracing::error!, but \
             the caller can invoke it without a count bound and thereby drive unbounded writes \
             through the host tracing/file-log pipeline",
        ),
    },
    CommandOverride {
        command: "set_remote_agent_conversation_muted",
        policy: policy(
            RiskClass::AgentControl,
            NONE,
            "spawn-free direct twin: writes mute metadata after projecting workspace state through \
             the recovery-free response builder",
        ),
    },
    CommandOverride {
        command: "switch_remote_agent_conversation_persona",
        policy: policy(
            RiskClass::AgentControl,
            MUTATES_CONTENT,
            "spawn-free direct twin: rejects running agents and changes prompt content consumed \
             by the next turn",
        ),
    },
    CommandOverride { command: "request_remote_conversation_archive", policy: policy(RiskClass::AgentControl, SEEDS_STATE, "spawn-free twin persists a host archive lifecycle intent") },
    CommandOverride { command: "request_remote_conversation_fork", policy: policy(RiskClass::AgentControl, CONTENT_AND_SEEDS, "spawn-free twin preallocates a child id and persists a host fork lifecycle intent") },
    CommandOverride { command: "get_remote_conversation_lifecycle_request", policy: policy(RiskClass::Read, NONE, "pure lifecycle intent repository read") },
    process_refusal(
        "set_agent_conversation_muted",
        "detector-c, hand-traced: the mute=true path calls agent_workspace_response_for_state, \
         which schedules PR supervision recovery; recover_agent_workspace_pr_supervision can \
         reach GitService::get_head_sha and can resume agent repair/publication work before the \
         mute metadata row is written; set_remote_agent_conversation_muted is the spawn-free twin",
    ),
];

/// PR 3.1-b batch 9 ITEM 0 — a detector-(c) refusal, declared at the capability it reaches.
///
/// `Elevated` is the only class `class_permits` accepts `SpawnsProcess` under, and v1 excludes
/// `ui:elevated`, so this pair is exactly `V1Resolution::HostDeniedSpawnsProcess`. It is an
/// authority-INCREASING correction in every case: all fourteen sat at the `AgentControl` module
/// default, which understated them.
/// A batch-13 `Read` row: registered on a body audit that found no repository write.
const fn read_row(command: &'static str, reason: &'static str) -> CommandOverride {
    CommandOverride {
        command,
        policy: policy(RiskClass::Read, NONE, reason),
    }
}

const fn process_refusal(command: &'static str, reason: &'static str) -> CommandOverride {
    CommandOverride {
        command,
        policy: policy(RiskClass::Elevated, PROCESS, reason),
    }
}

/// A per-command audit refusal that no v1 scope can accommodate.
///
/// Distinct from a `CommandOverride`: an override states what authority a command CARRIES, and
/// this states what a hand audit FOUND. Keeping them apart is what stops a classification being
/// bought by overstating a class — the `reopen_issue` pin's standing warning against putting a
/// false statement in the ledger to buy a manifest row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditRefusal {
    pub command: &'static str,
    pub reason: AuditRefusalReason,
    /// The finding, specific enough to falsify. `batch9_audit_refusals_are_tied_to_a_live_pin`
    /// additionally requires the command to be named inside a pinned-refusal test, so a row
    /// here cannot exist without the mechanism being asserted somewhere that CI runs.
    pub finding: &'static str,
    pub batch: &'static str,
}

/// Commands the class/capability arithmetic would admit, refused by a recorded audit.
///
/// **The bar, and what it excludes.** A row belongs here only if the finding disqualifies the
/// command at EVERY v1 scope as it stands. It is NOT enough that a batch declined to register
/// it. In particular "detector (a) fires", "detector (b) fires", "it writes", and "it steers"
/// are all excluded: the facade serves 16 `agentControl` ops today, four carrying
/// `Capability::AgentControl` and three carrying `Capability::SeedsSpawnTriggeringState`, so
/// arming authority demonstrably does NOT foreclose v1 exposure. Refusals of that shape stay
/// `registerable` and stay on the ratchet until someone does the `ui:agent` audit they need.
///
/// That is why the batch-9 sweep classified 25 of the 50 audited-and-refused commands and left
/// 25 alone; the excluded ones are listed in `batch9_arming_and_write_refusals_stay_on_the_ratchet`
/// so a later batch does not quietly reach for a class this table deliberately does not offer.
pub const AUDIT_REFUSALS: &[AuditRefusal] = &[
    // --- Fail-open: a host failure is rendered as absence or success, so no scope serves it
    //     honestly. The unlocking fix is named in each finding.
    AuditRefusal {
        command: "get_pending_permissions",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "returns Ok(vec![]) when the repository read fails, so an outage is \
                  indistinguishable from `no gates are open`; fix by propagating the error",
        batch: "1-2",
    },
    AuditRefusal {
        command: "get_pending_questions",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "same Ok(vec![]) shape as get_pending_permissions; fix by propagating the error",
        batch: "1-2",
    },
    AuditRefusal {
        command: "list_agent_composer_skills",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "agent_composer_commands/skills.rs:766 swallows the Codex config read, so \
                  losing it reports DISABLED skills as enabled — a fail-open that changes the \
                  answer, not just its completeness (also :299/:318/:442/:589)",
        batch: "4, re-audited 8",
    },
    AuditRefusal {
        command: "set_active_plan",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "swallows TWO errors — the execution-plan lookup is `if let Ok(Some(ep))` and \
                  the follow-up set_execution_plan_id write is discarded with `let _ =` — so a \
                  partial write returns Ok(()) while the execution-plan id the Kanban/Graph \
                  filters and the scheduler read silently did not move",
        batch: "7",
    },
    // --- Spawn-capable machinery constructed to serve a read. Unlockable by the same kind of
    //     read-only seam extraction that produced the registered `list_remote_*` reads.
    AuditRefusal {
        command: "get_agent_run_status_unified",
        reason: AuditRefusalReason::ConstructsSpawnCapableService,
        finding: "takes execution_state and tauri::AppHandle and calls create_chat_service \
                  (unified_chat_commands/mod.rs:9657) purely to reach one getter, handing a \
                  read-scoped caller a constructed steer surface",
        batch: "8",
    },
    AuditRefusal {
        command: "get_queued_agent_messages",
        reason: AuditRefusalReason::SeamResolvedViaRemoteTwin,
        finding: "Wave B3a split the corrected unified_chat_commands/mod.rs:4516 carrier seam; \
                  list_remote_queued_agent_messages is the registered spawn-free answer through \
                  list_queued_agent_messages_for_state, while the local command deliberately \
                  keeps its ChatService path, so registering both names would duplicate one query",
        batch: "8, resolved B3a",
    },
    // --- Answered by a registered remote twin through a deliberately split seam.
    AuditRefusal {
        command: "list_agent_conversations",
        reason: AuditRefusalReason::SeamResolvedViaRemoteTwin,
        finding: "batch 5 split the seam; list_remote_agent_conversations is the registered \
                  answer and the local twin deliberately stays off the facade, so registering \
                  this name would put two facade paths on one query for no new capability",
        batch: "5, re-affirmed 8",
    },
    AuditRefusal {
        command: "list_agent_conversations_page",
        reason: AuditRefusalReason::SeamResolvedViaRemoteTwin,
        finding: "same split seam; list_remote_agent_conversations_page is the registered answer",
        batch: "5, re-affirmed 8",
    },
    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 10 — the twin surfaces.
    //
    // Batch 9's bar is unchanged: a row belongs here only if the finding disqualifies the command
    // at EVERY v1 scope. These four clear it for a reason that has nothing to do with arming —
    // the facade ALREADY answers each query, so a second name would add a facade path and no
    // capability. That is the batch-5/8 precedent (`list_agent_conversations`), applied to the
    // three transcript reads batch 4 split and to the permission gate batch 1.5 split.
    // -----------------------------------------------------------------------------------
    AuditRefusal {
        command: "get_agent_conversation",
        reason: AuditRefusalReason::SeamResolvedViaRemoteTwin,
        finding: "batch 4 split the seam; both this command and the registered \
                  get_remote_agent_conversation delegate to the SAME \
                  get_agent_conversation_for_app_state seam and return the same payload, so the \
                  only difference is that the local name first calls \
                  wake_agent_workspace_for_bridge_events — whose error it discards with \
                  tracing::warn! and reads anyway. Registering it would put two facade paths on \
                  one query while dragging the wake's steer sink onto the facade for no payload",
        batch: "4, classified 10",
    },
    AuditRefusal {
        command: "get_agent_conversation_messages_page",
        reason: AuditRefusalReason::SeamResolvedViaRemoteTwin,
        finding: "same split seam and the same discarded wake; \
                  get_remote_agent_conversation_messages_page is the registered answer and \
                  applies identical limit clamping (unwrap_or(40).clamp(1, 200))",
        batch: "4, classified 10",
    },
    AuditRefusal {
        command: "get_agent_conversation_timeline_page",
        reason: AuditRefusalReason::SeamResolvedViaRemoteTwin,
        finding: "same split seam and the same discarded wake; \
                  get_remote_agent_conversation_timeline_page is the registered answer",
        batch: "4, classified 10",
    },
    // The sharpest twin in the census, and the reason it is a DENIAL rather than a scope call:
    // the facade does not merely answer this query elsewhere, it already registers this exact
    // Rust fn twice. `approve_permission_request` and `deny_permission_request` are not separate
    // functions — both rows target `permission_commands::resolve_permission_request` and differ
    // only in a server-minted `pins: [("args", "decision", ...)]`. The Operate/AgentControl split
    // between them is therefore enforced by the COMMAND NAME, which the client cannot choose the
    // semantics of. Registering the raw name would hand the client the `decision` field, so a
    // device holding only the default `ui:operate` grant could send `"decision": "allow"` and
    // reach the AgentControl-gated authorize-a-live-tool-call effect. There is no v1 scope at
    // which that is servable: at `ui:operate` it is an escalation, and at `ui:agent` it is a
    // duplicate of a row that already exists.
    AuditRefusal {
        command: "resolve_permission_request",
        reason: AuditRefusalReason::SeamResolvedViaRemoteTwin,
        finding: "the facade already registers this exact fn twice — approve_permission_request \
                  (AgentControl) and deny_permission_request (Operate) both target it with the \
                  decision field server-pinned. Registering the raw name would move branch \
                  selection from the command name to a client-supplied argument and collapse the \
                  Operate/AgentControl split that pinning exists to enforce",
        batch: "1.5 split, classified 10",
    },
    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 10 — the writes whose `ui:agent` audit FAILED.
    //
    // These three were on the ratchet as "repository write behind a status guard, writer audit
    // never done". Batch 10 did the audit, and it came back dirty. Recorded here rather than
    // registered, and deliberately NOT recorded as arming findings — each is a host failure or a
    // missing effect rendered to the caller as success, which is the fail-open shape no scope
    // serves honestly.
    // -----------------------------------------------------------------------------------
    AuditRefusal {
        command: "archive_tasks_in_group",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "task_commands/mutation.rs:2242 swallows each per-task archive error with \
                  tracing::warn! and continues the loop, so the command returns \
                  Ok(BulkArchiveResponse { archived_count }) with a count silently short of the \
                  group and no way for the caller to learn which tasks survived. Compounded by \
                  the standing authority-OBSCURING finding: archive writes only archived_at, \
                  there is no InternalStatus::Archived, and get_by_status filters \
                  `archived_at IS NULL`, so a partially-failed sweep leaves Executing tasks \
                  holding their agent process while invisible to the reconciler; fix by \
                  propagating the per-task error",
        batch: "3 refused, audited and classified 10",
    },
    AuditRefusal {
        command: "request_task_changes_from_reviewing",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "the idempotency-flag write degrades destructively: review_commands.rs:675 reads \
                  the task's metadata with parse_metadata(&task).unwrap_or_else(|| json!({})) and \
                  :683 re-serialises it with unwrap_or_else(|_| r#\"{\\\"request_changes_\
                  initiated\\\":true}\"#), so an unparseable or unserialisable blob is REPLACED by \
                  a stub and every other metadata field is dropped — while the command returns \
                  Ok. Its sibling request_task_changes_for_review reaches the same \
                  RevisionNeeded transition with no such write and is registered instead; fix by \
                  propagating both serde errors",
        batch: "audited and classified 10",
    },
    AuditRefusal {
        command: "skip_qa",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "the command promises a QA bypass and does not deliver one: it writes every step \
                  as QAStepResult::skipped, but QAResults::from_results derives Passed only when \
                  passed_steps == total_steps and skipped steps increment skipped_steps, so \
                  overall_status resolves to Pending, not Passed — contradicting the body's own \
                  `// Mark all steps as passed (skipped behavior)` comment. A caller is told the \
                  skip succeeded while the verdict it wanted was never written; fix by deciding \
                  the intended verdict and making from_results express it",
        batch: "7 refused, audited and classified 10",
    },
    // -----------------------------------------------------------------------------------
    // PR 3.1-b batch 11 — census B4 remainder, the seven whose audit came back dirty.
    //
    // Every one of these is a fail-open in the strict sense the vocabulary requires: an error
    // is discarded on a path that still returns `Ok`, and the discarded error CHANGES the
    // answer rather than merely truncating it. Members whose only defect was non-atomicity are
    // NOT here — `set_default_workflow` and `activate_methodology` propagate their errors and
    // are registered with the product bug recorded on the ledger row instead.
    // -----------------------------------------------------------------------------------
    AuditRefusal {
        command: "analyze_dependencies",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "ideation_commands_dependencies.rs:117 downgrades the \
                  set_dependencies_acknowledged write to a tracing::warn! and returns \
                  Ok(DependencyGraphResponse), so the accept-gate flag the command exists to set \
                  can silently stay unset. The command is not the pure read its name implies — \
                  viewing the graph IS the write, and the write is the part that can vanish",
        batch: "audited and classified 11",
    },
    AuditRefusal {
        command: "export_ideation_session",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "session_export_service.rs:291/295/299 decode proposal steps, \
                  acceptance_criteria and priority_factors with serde_json::from_str(..).ok(), \
                  so a column that fails to parse is exported as absent rather than as an error \
                  — the artifact silently loses the fields a re-import would rebuild tasks from. \
                  Compounded at :411, where a detected cycle replaces the whole plan version \
                  history with Ok(vec![])",
        batch: "audited and classified 11",
    },
    AuditRefusal {
        command: "create_task_proposal",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "ideation_commands_proposals.rs:38 coerces an unparsable client priority to \
                  Priority::Medium instead of rejecting it, and :42/:45/:48 turn a failed \
                  serde_json::to_string into an empty string — including affected_paths, the \
                  exact value validate_affected_paths_json is later supposed to check, so a \
                  serialization failure silently produces an empty path set that passes \
                  validation. helpers.rs:462 additionally swallows set_dependencies_acknowledged \
                  after the proposal INSERT has already committed",
        batch: "audited and classified 11",
    },
    // `get_agent_harness_availability` and `get_ideation_harness_availability` carried
    // batch-11 FailOpenUntilFixed audit refusals here (the shared helper discards the
    // lane-settings read with .ok().flatten() at ideation_harness_availability.rs:344/:360, so
    // a DB error is indistinguishable from 'no row configured' and a broken lane can report
    // fully green). That finding is still true — but main's #976 probe cache moved both
    // commands to the process floor, and the mechanical HostDeniedSpawnsProcess resolution
    // supersedes the audit row: batch 9 requires audit rows only where the mechanical
    // resolution would otherwise be Registerable. The fail-open stays recorded on their
    // process_refusal rows' cluster comment and in the batch-11 table.
    AuditRefusal {
        command: "create_cross_project_session",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "plan_reference_import.rs:239 swallows artifact_repo.add_relation after the \
                  cloned plan artifact at :233 has already been persisted, so the imported plan \
                  loses its derived_from provenance edge while the command returns Ok — and \
                  provenance is the whole point of a cross-project import. The durable \
                  external_events row is discarded the same way at \
                  ideation_commands_cross_project.rs:242",
        batch: "audited and classified 11",
    },
    AuditRefusal {
        command: "import_ideation_session",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "session_export_service.rs:671/675/679 re-serialize proposal steps, \
                  acceptance_criteria and priority_factors with .ok() INSIDE the committed \
                  import transaction, so those columns land NULL while ImportedSession.\
                  proposal_count still reports the proposal as fully imported. The input \
                  validation front end is strong (size cap, schema-version pin, cycle and bounds \
                  checks); the loss is entirely on the write side",
        batch: "audited and classified 11",
    },
    // --- PR 3.1-b batch 14's seven `TransportShapeDeferred` rows are GONE (WP4 (a)).
    //
    // They recorded one shared mechanism — "AppError derives only `Error, Debug`, so the
    // macro's `fallible` arm cannot render it" — and the mechanism never existed.
    // `ralphx_domain::error` has carried a hand-written `impl Serialize for AppError` since
    // `96ce527a9`; it has to, because Tauri itself requires `Serialize` on a command's error
    // type. The facade was already dispatching two `AppResult`-returning commands from the very
    // module the block cited (`create_task_step`, `update_task_step`). All eight rows (the seven
    // here plus batch 8's `list_conversation_folder_references`) are registered instead, and
    // `the_transport_shape_refusal_premise_stays_disproven` pins the disproof so the class
    // cannot be reconstituted on the same false finding.
    // --- The one fail-open in the final batch, in the shape `list_agent_composer_skills` set.
    AuditRefusal {
        command: "get_manual_role_defaults",
        reason: AuditRefusalReason::FailOpenUntilFixed,
        finding: "manual_role_default_commands.rs:539-546 turns a resolution Err into \
                  `effective: None` PLUS an assumed AgentHarnessKind::Claude provider, and \
                  control_options at :558 then computes capability and speed availability \
                  AGAINST that fabricated default. The caller receives a plausible \
                  enabled/disabled control set derived from an error rather than from \
                  configuration — an outage changes the ANSWER, not just its completeness. Its \
                  two sibling reads take a different path and are registered; fix by \
                  propagating the resolution error",
        batch: "14",
    },
    // --- The NEW reason. See the AuditRefusalReason::ReachesCorrectiveTransition doc for why
    //     minting it beat filing a false one, and why it is not the generic arming code the
    //     vocabulary still withholds.
    AuditRefusal {
        command: "reject_fix_task",
        reason: AuditRefusalReason::ReachesCorrectiveTransition,
        finding: "review_commands.rs:273 calls transition_task_corrective(fix_task, Failed) and \
                  :297 calls it again for the original task (Backlog) once max attempts are \
                  exceeded. Corrective jumps are the repair-path-only state-machine escape and \
                  `no_registered_facade_target_reaches_a_corrective_transition` forbids a \
                  registered target from reaching one at ANY scope, so this is a hard invariant \
                  rather than a scope call. Held unclassified since batch 10 for want of an \
                  honest code; classified here rather than mis-filed. The rest of the body is \
                  clean, so the finding is precisely the corrective reach — fix by routing the \
                  rejection through a mediator that pins its own target, as move_task does",
        batch: "10 refused, classified 14",
    },
];

/// The audit refusal recorded for a command, if any.
pub fn audit_refusal_for(command: &str) -> Option<AuditRefusal> {
    AUDIT_REFUSALS
        .iter()
        .find(|entry| entry.command == command)
        .copied()
}

pub const AUTHORITY_REDUCING_EXEMPTIONS: &[AuthorityReducingExemption] = &[
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
    // WP2 — the remote agent brake. Appended after the batch-3 rows; the exactness test
    // filters by `kind`/`subject` rather than by index, so appending is safe.
    AuthorityReducingExemption {
        subject: "request_remote_agent_stop",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "commands/remote_agent_stop_commands.rs persists one conversation-scoped stop intent and nothing else; the only reader is application/startup_background.rs::drain_one_remote_agent_stop, which calls ChatService::stop_agent and can therefore only END an agent run — there is no path from the row to a start, resume, or content write, and the row names no process",
    },
    AuthorityReducingExemption {
        subject: "cancel_remote_queued_agent_message",
        kind: "command",
        direction: "authority-reducing",
        scope: "ui:operate",
        rationale: "commands/remote_queue_commands.rs validates an active Project conversation and only removes the named queued turn from durable storage and memory; it cannot create content, start, resume, steer, or dispatch an agent",
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
/// — so a paired phone could sweep every project on the host in one call. A remote
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
    // live agent would otherwise never be proved unreachable without `ui:agent` —
    // the one class of member the generator cannot find by itself.
    ("send_remote_chat_message", "steers-live-agent-turn"),
    // PR 3.1-b batch 10 — the two registered arming writes NO detector models.
    //
    // Same reasoning as `send_remote_chat_message` above, and the same failure it prevents. The
    // P-17b negative suite is generated from `agent_control_floor` (detector (a) ∪ detector (b))
    // ∪ `declared_memberships`. Both commands arm through a surface neither detector watches —
    // `update_qa_settings` writes an in-memory `RwLock`, not a repository, and
    // `set_active_project` writes `ExecutionState` atomics rather than an `InternalStatus` — so
    // each is detector-silent and would otherwise be REGISTERED at `ui:agent` while never being
    // proved unreachable from an explicit `ui:read`+`ui:operate` grant. That is precisely the
    // hole the chat-send row exists to close, and registering an arming write without one would
    // reopen it.
    //
    // Appended, never inserted: `exemptions_and_declared_memberships_are_exact` indexes
    // `DECLARED_MEMBERSHIPS[0]` and `[1]` positionally.
    ("update_qa_settings", "arms-auto-qa"),
    ("set_active_project", "arms-scheduler-quota"),
    // PR 3.1-b batch 11 — two more detector-silent arming writes, same reasoning as above.
    //
    // `update_ideation_settings` writes a plain settings row and `update_agent_lane_settings`
    // writes a plain lane row, so detector (b) — which watches SPAWN_TRIGGERING_STATE_SURFACE —
    // is silent on both. But the hand audit found a spawner on the other end of each: the plan
    // verification service gates on `auto_verify_draft_plans`, and `resolve_agent_spawn_settings`
    // reads the lane row to choose the harness a live agent is launched with. Registering either
    // without a declaration would put an arming write at `ui:agent` that the generated P-17b
    // negative suite never proves unreachable without `ui:agent`.
    ("update_ideation_settings", "arms-auto-plan-verification"),
    ("update_agent_lane_settings", "arms-agent-spawn-harness"),
    // PR 3.1-b batch 12 — three automation writes that flip `automations.status` to Active.
    //
    // Active is the armed value the `automation-active` state surface already names, and
    // `spawn_automation_scheduler` is already listed as its reading loop — so unlike the batch-10
    // and batch-11 declarations, the SURFACE here is modelled and only the WRITE is invisible.
    // Detector (b) matches a write by marker, and that surface carries exactly one
    // (`reopen_run_corrective`); none of these three routes through it. Widening the marker list
    // to catch them was rejected: markers are matched against every command's closure, so a
    // broader marker changes the floor for members other batches already dispositioned. A
    // declaration states the finding without moving anyone else's measurement.
    //
    // `resume_automation_run` is deliberately NOT here — it does carry the marker, detector (b)
    // fires, and it takes the capability on the detector's own evidence.
    // PR 3.1-b batch 13 — five deferred-authority writes registered at `ui:agent`.
    //
    // The census B6 plan asks every member "does this write change what a FUTURE agent process
    // may do?" and answers yes with `ConfiguresFutureProcessAuthority`. That capability is
    // admitted by `class_permits` ONLY under `Elevated`, which v1 grants no scope for, so taking
    // the plan literally would convert five audited-clean bounded writes into deferrals by
    // notation rather than by finding. `update_agent_lane_settings` already settled the idiom:
    // it picks the harness, model and effort a live agent is launched with — strictly more
    // deferred authority than any row here — and records that as AgentControl plus a declaration.
    (
        "update_mcp_server_override",
        "configures-future-agent-tool-authority",
    ),
    (
        "clear_mcp_server_override",
        "configures-future-agent-tool-authority",
    ),
    (
        "update_mcp_tool_override",
        "configures-future-agent-tool-authority",
    ),
    (
        "clear_mcp_tool_override",
        "configures-future-agent-tool-authority",
    ),
    (
        "update_ui_feature_flags",
        "configures-future-agent-capability-gates",
    ),
    ("restart_automation", "arms-automation-scheduler"),
    ("retry_automation_plan_judge", "arms-automation-scheduler"),
    ("skip_automation_judge", "arms-automation-scheduler"),
    // PR 3.1-b batch 14 — the final batch's two BOUNDED deferred-authority writes.
    //
    // Both pick which model/effort a LATER agent process runs with, and neither is watched by a
    // detector: `update_workspace_review_runtime_settings` writes a plain settings row and
    // `upsert_custom_agent_model` writes a registry row, so detector (b) is silent on each and
    // the P-17b negative suite would otherwise never prove them unreachable from a default
    // pairing. Same reasoning as the batch 10/11 rows above.
    //
    // These stay REGISTERABLE rather than becoming Elevated deferrals because the authority
    // they configure is bounded — which model runs — whereas this batch's three Elevated rows
    // configure the containment boundary itself (sandbox_mode/approval_policy, MCP filesystem
    // read roots). That is the whole of the distinction, recorded here so a successor does not
    // read the two groups as inconsistent.
    (
        "update_workspace_review_runtime_settings",
        "configures-future-agent-runtime",
    ),
    (
        "upsert_custom_agent_model",
        "configures-future-agent-runtime",
    ),
    // WP1 (remote conversation continuation). Same reasoning as `send_remote_chat_message`
    // above and the same failure it prevents: the P-17b negative suite is generated from
    // detector output, and a spawn-free command that CAUSES a live agent turn would otherwise
    // never be proved unreachable from a default `ui:read`+`ui:operate` pairing. Detector (b)
    // does fire on this one via the `remote-conversation-message` surface row, but the
    // declaration is recorded anyway because the membership is a property of the command, not
    // of whichever detector happens to model the surface this month.
    (
        "request_remote_agent_conversation_message",
        "seeds-agent-turn-for-idle-conversation",
    ),
    // WP5a (remote conversation mode switch). Same reasoning as the continuation row above: the
    // P-17b negative suite is generated from detector output, and a spawn-free command that
    // causes the host to PREPARE A WORKSPACE — a git worktree a later agent process runs in —
    // would otherwise never be proved unreachable from a default `ui:read`+`ui:operate` pairing.
    // Detector (b) does fire on this one via the `remote-conversation-mode-switch` surface row,
    // but the declaration is recorded anyway because the membership is a property of the command,
    // not of whichever detector happens to model the surface this month.
    (
        "request_remote_agent_conversation_mode_switch",
        "prepares-workspace-for-later-agent-run",
    ),
    ("resolve_remote_user_question", "steering-question"),
    (
        "request_remote_execution_resume",
        "resumes-execution-through-host-dispatcher",
    ),
    (
        "request_remote_task_resume",
        "resumes-task-through-host-dispatcher",
    ),
    (
        "request_remote_task_restart",
        "restarts-task-through-host-dispatcher",
    ),
    (
        "request_remote_group_resume",
        "resumes-task-group-through-host-dispatcher",
    ),
    (
        "request_remote_recovery_prompt_resolution",
        "resolves-recovery-through-host-dispatcher",
    ),
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
