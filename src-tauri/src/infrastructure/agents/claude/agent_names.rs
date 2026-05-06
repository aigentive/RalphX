// Centralized agent name constants
//
// Single source of truth for all fully-qualified agent names used throughout
// the RalphX Rust backend. These match the `ralphx:<name>` format where `<name>`
// is the canonical agent id from `agents/*/agent.yaml`.
//
// If an agent is renamed in canonical config, update ONLY this file.

/// Plugin prefix for all RalphX agents
pub const PLUGIN_PREFIX: &str = "ralphx:";

// ── Short names (without "ralphx:" prefix) ───────────────────────────────
// Used by AGENT_CONFIGS in agent_config/ and MCP authorization resolution.
// These match the canonical agent ids in `agents/*/agent.yaml`.

pub const SHORT_ORCHESTRATOR_IDEATION: &str = "ralphx-ideation";
pub const SHORT_ORCHESTRATOR_IDEATION_READONLY: &str = "ralphx-ideation-readonly";
pub const SHORT_SESSION_NAMER: &str = "ralphx-utility-session-namer";
pub const SHORT_PR_DESCRIBER: &str = "ralphx-utility-pr-describer";
pub const SHORT_CHAT_TASK: &str = "ralphx-chat-task";
pub const SHORT_CHAT_PROJECT: &str = "ralphx-chat-project";
pub const SHORT_REVIEW_CHAT: &str = "ralphx-review-chat";
pub const SHORT_REVIEW_HISTORY: &str = "ralphx-review-history";
pub const SHORT_WORKER: &str = "ralphx-execution-worker";
pub const SHORT_CODER: &str = "ralphx-execution-coder";
pub const SHORT_GENERAL_EXPLORER: &str = "ralphx-general-explorer";
pub const SHORT_GENERAL_WORKER: &str = "ralphx-general-worker";
pub const SHORT_AGENT_WORKSPACE_REPAIR: &str = "ralphx-agent-workspace-repair";
pub const SHORT_REVIEWER: &str = "ralphx-execution-reviewer";
pub const SHORT_QA_PREP: &str = "ralphx-qa-prep";
pub const SHORT_QA_EXECUTOR: &str = "ralphx-qa-executor";
pub const SHORT_ORCHESTRATOR: &str = "ralphx-execution-orchestrator";
pub const SHORT_DEEP_RESEARCHER: &str = "ralphx-research-deep-researcher";
pub const SHORT_PROJECT_ANALYZER: &str = "ralphx-project-analyzer";
pub const SHORT_MERGER: &str = "ralphx-execution-merger";
pub const SHORT_MEMORY_MAINTAINER: &str = "ralphx-memory-maintainer";
pub const SHORT_MEMORY_CAPTURE: &str = "ralphx-memory-capture";

// ── Plan verification critic agents ─────────────────────────────────────
pub const SHORT_PLAN_CRITIC_COMPLETENESS: &str = "ralphx-plan-critic-completeness";
pub const SHORT_PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY: &str =
    "ralphx-plan-critic-implementation-feasibility";
pub const SHORT_PLAN_VERIFIER: &str = "ralphx-plan-verifier";

// ── Team lead variants (extends base agents) ────────────────────────────
pub const SHORT_IDEATION_TEAM_LEAD: &str = "ralphx-ideation-team-lead";
pub const SHORT_WORKER_TEAM: &str = "ralphx-execution-team-lead";
pub const SHORT_IDEATION_TEAM_MEMBER: &str = "ideation-team-member";

// ── Ideation specialist agents (spawned by ralphx-ideation-team-lead) ───────────
pub const SHORT_IDEATION_SPECIALIST_BACKEND: &str = "ralphx-ideation-specialist-backend";
pub const SHORT_IDEATION_SPECIALIST_FRONTEND: &str = "ralphx-ideation-specialist-frontend";
pub const SHORT_IDEATION_SPECIALIST_INFRA: &str = "ralphx-ideation-specialist-infra";
pub const SHORT_IDEATION_SPECIALIST_UX: &str = "ralphx-ideation-specialist-ux";
pub const SHORT_IDEATION_SPECIALIST_CODE_QUALITY: &str = "ralphx-ideation-specialist-code-quality";
pub const SHORT_IDEATION_ADVOCATE: &str = "ralphx-ideation-advocate";
pub const SHORT_IDEATION_CRITIC: &str = "ralphx-ideation-critic";
pub const SHORT_IDEATION_SPECIALIST_PIPELINE_SAFETY: &str =
    "ralphx-ideation-specialist-pipeline-safety";
pub const SHORT_IDEATION_SPECIALIST_STATE_MACHINE: &str =
    "ralphx-ideation-specialist-state-machine";

// ── ChatService team agents (team_mode=true → resolve_agent_with_team_mode) ──

/// Ideation team lead (ChatContextType::Ideation + team_mode)
pub const AGENT_IDEATION_TEAM_LEAD: &str = "ralphx:ralphx-ideation-team-lead";

/// Worker team lead (ChatContextType::TaskExecution + team_mode)
pub const AGENT_WORKER_TEAM: &str = "ralphx:ralphx-execution-team-lead";

// ── ChatService agents (resolve_agent → build_command → --agent flag) ─────

/// Ideation orchestrator (default for ChatContextType::Ideation)
pub const AGENT_ORCHESTRATOR_IDEATION: &str = "ralphx:ralphx-ideation";

/// Ideation orchestrator in read-only mode (session status = "accepted")
pub const AGENT_ORCHESTRATOR_IDEATION_READONLY: &str = "ralphx:ralphx-ideation-readonly";

/// Plan verifier agent (ChatContextType::Ideation when session_purpose = Verification)
pub const AGENT_PLAN_VERIFIER: &str = "ralphx:ralphx-plan-verifier";

/// Task-scoped chat (ChatContextType::Task)
pub const AGENT_CHAT_TASK: &str = "ralphx:ralphx-chat-task";

/// Project-scoped chat (ChatContextType::Project)
pub const AGENT_CHAT_PROJECT: &str = "ralphx:ralphx-chat-project";

/// General read-only project explorer for project-scoped agent conversations
pub const AGENT_GENERAL_EXPLORER: &str = "ralphx:ralphx-general-explorer";

/// General edit worker for project-scoped agent conversations
pub const AGENT_GENERAL_WORKER: &str = "ralphx:ralphx-general-worker";

/// Agent-workspace publish repair agent
pub const AGENT_WORKSPACE_REPAIR: &str = "ralphx:ralphx-agent-workspace-repair";

/// Worker execution agent (ChatContextType::TaskExecution)
pub const AGENT_WORKER: &str = "ralphx:ralphx-execution-worker";

/// Delegated coding execution agent (invoked by worker orchestration)
pub const AGENT_CODER: &str = "ralphx:ralphx-execution-coder";

/// Reviewer agent (ChatContextType::Review, fresh review cycle)
pub const AGENT_REVIEWER: &str = "ralphx:ralphx-execution-reviewer";

/// Merger agent (ChatContextType::Merge)
pub const AGENT_MERGER: &str = "ralphx:ralphx-execution-merger";

/// Review-chat agent (ChatContextType::Review when status = "review_passed")
pub const AGENT_REVIEW_CHAT: &str = "ralphx:ralphx-review-chat";

/// Review-history agent (ChatContextType::Review when status = "approved")
pub const AGENT_REVIEW_HISTORY: &str = "ralphx:ralphx-review-history";

// ── Fire-and-forget agents (spawn_agent path) ────────────────────────────

/// Session naming agent (haiku, background)
pub const AGENT_SESSION_NAMER: &str = "ralphx:ralphx-utility-session-namer";

/// Agent workspace PR description writer (background)
pub const AGENT_PR_DESCRIBER: &str = "ralphx:ralphx-utility-pr-describer";

/// Project analysis agent (background)
pub const AGENT_PROJECT_ANALYZER: &str = "ralphx:ralphx-project-analyzer";

// ── QA agents (spawner path from state machine) ──────────────────────────

/// QA preparation agent (background, spawned on Ready state entry)
pub const AGENT_QA_PREP: &str = "ralphx:ralphx-qa-prep";

/// QA refiner agent (spawned on QaRefining state entry)
/// Note: No dedicated plugin agent file — uses default Claude behavior
pub const AGENT_QA_REFINER: &str = "ralphx:qa-refiner";

/// QA tester agent (spawned on QaTesting state entry)
/// Note: No dedicated plugin agent file — uses default Claude behavior
pub const AGENT_QA_TESTER: &str = "ralphx:qa-tester";

/// Map a state-machine spawner agent type string to the correct FQ agent name.
///
/// The state machine uses short identifiers ("qa-prep", "qa-refiner", "qa-tester")
/// when calling `agent_spawner.spawn()`. Tests also pass "worker" and "reviewer".
/// This maps them to their fully-qualified names that match the plugin's agent frontmatter.
pub fn spawner_agent_name(agent_type: &str) -> &'static str {
    match agent_type {
        // Production: QA agents via state machine side effects
        "qa-prep" => AGENT_QA_PREP,
        "qa-refiner" => AGENT_QA_REFINER,
        "qa-tester" => AGENT_QA_TESTER,
        // Test/fallback: worker, reviewer, merger via spawner tests and mocks
        "worker" | "ralphx-execution-worker" => AGENT_WORKER,
        "coder" | "ralphx-execution-coder" => AGENT_CODER,
        "reviewer" | "ralphx-execution-reviewer" => AGENT_REVIEWER,
        "merger" | "ralphx-execution-merger" => AGENT_MERGER,
        // Fallback: qualify using prefix
        other => {
            tracing::warn!(
                agent_type = other,
                "Unknown spawner agent type, falling back to qualify_agent_name"
            );
            Box::leak(super::qualify_agent_name(other).into_boxed_str())
        }
    }
}
