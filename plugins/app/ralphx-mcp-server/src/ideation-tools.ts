/**
 * Ideation-family MCP tool definitions
 */

import { Tool } from "@modelcontextprotocol/sdk/types.js";
import { buildRuntimeIdentityTransportHeaders } from "./runtime-context.js";
import type { TauriCallOptions } from "./tauri-client.js";

type TauriPost = (
  path: string,
  body: Record<string, unknown>,
  options?: TauriCallOptions,
) => Promise<unknown>;

export type DelegateContextRuntimeContext = {
  conversationId?: string;
  agentRunId?: string;
};

export async function callGetParentContextTool(
  callTauri: TauriPost,
  args: Record<string, unknown>,
  runtimeContext: DelegateContextRuntimeContext,
): Promise<unknown> {
  return callTauri("coordination/delegate/parent-context", args, {
    headers: buildRuntimeIdentityTransportHeaders(runtimeContext),
  });
}

export const IDEATION_TOOLS: Tool[] = [
  // ========================================================================
  // IDEATION TOOLS (ralphx-ideation agent)
  // ========================================================================
  {
    name: "create_task_proposal",
    description:
      "Create a new task proposal in the ideation session. Use this when the user describes a new feature, fix, or improvement they want to implement.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID (provided in context)",
        },
        title: {
          type: "string",
          description: "Clear, concise task title (e.g., 'Add dark mode toggle')",
        },
        description: {
          type: "string",
          description: "Detailed description of what needs to be done",
        },
        category: {
          type: "string",
          enum: ["setup", "feature", "fix", "refactor", "docs", "test", "performance", "security", "devops", "research", "design", "chore"],
          description: "Task category: setup (project init/infra), feature (new functionality), fix (bug fix), refactor (code restructure), docs (documentation), test (testing), performance (optimization), security (security hardening), devops (CI/CD/tooling), research (investigation/spike), design (UX/UI design), chore (maintenance/cleanup)",
        },
        priority: {
          type: "string",
          enum: ["critical", "high", "medium", "low"],
          description: "Suggested priority level. Default: medium",
        },
        steps: {
          type: "array",
          items: { type: "string" },
          description: "Step-by-step implementation plan. Each step should be a clear, actionable task (1-3 sentences). Typically 3-7 steps.",
        },
        acceptance_criteria: {
          type: "array",
          items: { type: "string" },
          description: "Testable criteria to verify task completion (e.g., 'API returns 200 with valid schema', 'All tests pass'). Typically 3-5 criteria.",
        },
        affected_paths: {
          type: "array",
          items: { type: "string" },
          description:
            "Coarse planned file or directory scope for this proposal. Prefer repo-relative paths or prefixes like 'src-tauri/src/http_server' or 'src/components/execution'. Use broad, credible boundaries rather than guessing an exact final file list. Required for implementation-affecting proposals; pure research/design proposals may omit it when no credible repo-change scope exists.",
        },
        target_project: {
          type: "string",
          description: "Optional: target project ID or filesystem path for cross-project ideation. Tag this proposal with the project it targets.",
        },
        depends_on: {
          type: "array",
          items: { type: "string" },
          description:
            "Optional proposal IDs that must be completed before this proposal. Use for staged or ordered work; omit only when the proposal is genuinely independent.",
        },
        expected_proposal_count: {
          type: "integer",
          description: "Total number of proposals you intend to create in this session. Required on every create_task_proposal call. First proposal locks the count; returns ready_to_finalize: true when proposal count matches expected_proposal_count — call finalize_proposals then.",
        },
      },
      required: ["session_id", "title", "category", "expected_proposal_count"],
    },
  },
  {
    name: "update_task_proposal",
    description:
      "Update an existing task proposal. Use when the user wants to modify a proposal's details, priority, or implementation plan.",
    inputSchema: {
      type: "object",
      properties: {
        proposal_id: {
          type: "string",
          description: "The proposal ID to update",
        },
        title: {
          type: "string",
          description: "Updated task title",
        },
        description: {
          type: "string",
          description: "Updated description",
        },
        category: {
          type: "string",
          enum: ["setup", "feature", "fix", "refactor", "docs", "test", "performance", "security", "devops", "research", "design", "chore"],
          description: "Updated category: setup (project init/infra), feature (new functionality), fix (bug fix), refactor (code restructure), docs (documentation), test (testing), performance (optimization), security (security hardening), devops (CI/CD/tooling), research (investigation/spike), design (UX/UI design), chore (maintenance/cleanup)",
        },
        user_priority: {
          type: "string",
          enum: ["critical", "high", "medium", "low"],
          description: "Updated priority level (overrides AI-suggested priority)",
        },
        steps: {
          type: "array",
          items: { type: "string" },
          description: "Updated implementation steps. Each step should be a clear, actionable task (1-3 sentences). Typically 3-7 steps.",
        },
        acceptance_criteria: {
          type: "array",
          items: { type: "string" },
          description: "Updated acceptance criteria. Testable criteria to verify task completion (e.g., 'API returns 200 with valid schema'). Typically 3-5 criteria.",
        },
        affected_paths: {
          type: "array",
          items: { type: "string" },
          description:
            "Updated coarse planned scope for the proposal. Use repo-relative path prefixes that bound the intended implementation area without pretending to know every final file.",
        },
        target_project: {
          type: "string",
          description: "Optional: set or update the target project for this proposal. Pass null or omit to leave unchanged.",
        },
        add_depends_on: {
          type: "array",
          items: { type: "string" },
          description:
            "Optional proposal IDs that this proposal should depend on. Adds dependency edges without replacing existing dependencies.",
        },
        add_blocks: {
          type: "array",
          items: { type: "string" },
          description:
            "Optional proposal IDs that this proposal should block. Adds reverse dependency edges without replacing existing dependencies.",
        },
      },
      required: ["proposal_id"],
    },
  },
  {
    name: "archive_task_proposal",
    description:
      "Archive a task proposal. Use when the user wants to remove a proposal that's no longer needed.",
    inputSchema: {
      type: "object",
      properties: {
        proposal_id: {
          type: "string",
          description: "The proposal ID to archive",
        },
      },
      required: ["proposal_id"],
    },
  },
  {
    name: "delete_task_proposal",
    description:
      "Delete a task proposal. Alias for archive_task_proposal — routes to the same endpoint. Use when the user or agent wants to delete/remove a proposal during ideation.",
    inputSchema: {
      type: "object",
      properties: {
        proposal_id: {
          type: "string",
          description: "The proposal ID to delete",
        },
      },
      required: ["proposal_id"],
    },
  },
  {
    name: "update_session_title",
    description:
      "Update the title of an ideation session or an agent conversation. Used by ralphx-utility-session-namer to persist auto-generated titles. Provide exactly one of session_id or conversation_id.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID to update",
        },
        conversation_id: {
          type: "string",
          description: "The agent conversation ID to update",
        },
        title: {
          type: "string",
          description: "The new title for the session or conversation (imperative mood, <=50 chars)",
        },
      },
      required: ["title"],
    },
  },
  {
    name: "list_session_proposals",
    description:
      "List all task proposals in an ideation session. Returns summary info (id, title, category, priority, dependencies). Use get_proposal for full details including steps and acceptance criteria.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "get_proposal",
    description:
      "Get full details of a task proposal including steps and acceptance criteria. Use after list_session_proposals to get complete information for a specific proposal.",
    inputSchema: {
      type: "object",
      properties: {
        proposal_id: {
          type: "string",
          description: "The proposal ID to fetch",
        },
      },
      required: ["proposal_id"],
    },
  },
  {
    name: "analyze_session_dependencies",
    description:
      "Get full dependency graph analysis including critical path, cycle detection, and blocking relationships. " +
      "Use to provide intelligent recommendations about proposal execution order. " +
      "Side effect: sets dependencies_acknowledged=true on the session, satisfying the finalize gate for multi-proposal sessions.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID to analyze",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "finalize_proposals",
    description:
      "Signal that all proposals and dependencies are complete. Validates expected count and applies all proposals to create tasks. Call this AFTER all create_task_proposal and update_task_proposal calls are done. " +
      "Gate: blocks with 400 if a multi-proposal session has not acknowledged dependencies (call analyze_session_dependencies, or set deps via create_task_proposal(depends_on) / update_task_proposal(add_depends_on/add_blocks)). " +
      "Response includes tasks_created (number of tasks created), message (null on success, error detail on gate block), and status (\"success\" when tasks were created normally, \"pending_acceptance\" when the confirmation gate is active and user must accept before tasks are created). " +
      "When status is \"pending_acceptance\": no tasks have been created yet — poll get_acceptance_status on each subsequent turn to check if user has accepted or rejected.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID",
        },
      },
      required: ["session_id"],
    },
  },

  // ========================================================================
  // ACCEPTANCE GATE TOOLS (ralphx-ideation)
  // ========================================================================
  {
    name: "get_acceptance_status",
    description:
      "Get the current acceptance_status for an ideation session. Use this to poll whether the user has accepted or rejected a pending finalize confirmation. " +
      "Call this on each subsequent turn after finalize_proposals returns status=\"pending_acceptance\". " +
      "Response includes session_id and acceptance_status (null = no pending confirmation, \"pending\" = waiting for user, \"accepted\" = user accepted — tasks were created, \"rejected\" = user rejected — you may re-finalize).",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID to check acceptance status for",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "get_pending_confirmations",
    description:
      "Get all ideation sessions that have a pending acceptance confirmation for the active project. " +
      "Use this at startup (Phase 0 RECOVER) to check if any sessions are awaiting user confirmation before proceeding. " +
      "Response includes a sessions array with session_id and session_title for each pending session.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },

  // ========================================================================
  // QUESTION TOOLS (ralphx-ideation agent — inline AskUserQuestion)
  // ========================================================================
  {
    name: "ask_user_question",
    description:
      "Ask the user one clarifying question, or provide questions[] to run a short interview from a single tool call. " +
      "Questions appear one at a time as inline cards in the chat. " +
      "Each question blocks until the user responds or skips it (up to 5 minutes). " +
      "Use for confirmations, multi-choice selections, or open-ended questions during ideation.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description:
            "The ideation session ID when explicitly provided. If omitted, RalphX uses the current runtime conversation/session context.",
        },
        question: {
          type: "string",
          description: "Single question text to display to the user. Omit when using questions[].",
        },
        header: {
          type: "string",
          description: "Optional header/title above the question (e.g., 'Confirm Plan')",
        },
        options: {
          type: "array",
          items: {
            type: "object",
            properties: {
              label: {
                type: "string",
                description: "Short label for the option (e.g., 'Yes', 'Option A')",
              },
              value: {
                type: "string",
                description: "Programmatic value returned when this option is selected. Defaults to label if omitted.",
              },
              description: {
                type: "string",
                description: "Optional longer description of what this option means",
              },
            },
            required: ["label"],
          },
          description: "Predefined answer options. If omitted, user can type a free-form response.",
        },
        multi_select: {
          type: "boolean",
          description: "If true and options are provided, user can select multiple options. Default: false.",
        },
        allow_skip: {
          type: "boolean",
          description: "If true, the user can skip the question. Default: true.",
        },
        metadata: {
          type: "object",
          description:
            "Optional UI metadata for RalphX-owned question affordances. Use sparingly for structured proposal flows.",
          additionalProperties: true,
        },
        questions: {
          type: "array",
          description: "Optional ordered interview questions. RalphX renders one question at a time and returns answers in order.",
          items: {
            type: "object",
            properties: {
              id: {
                type: "string",
                description: "Optional stable question identifier returned with the answer.",
              },
              question: {
                type: "string",
                description: "The question text to display to the user.",
              },
              header: {
                type: "string",
                description: "Optional header/title above this question.",
              },
              options: {
                type: "array",
                items: {
                  type: "object",
                  properties: {
                    label: {
                      type: "string",
                      description: "Short label for the option.",
                    },
                    value: {
                      type: "string",
                      description: "Programmatic value returned when this option is selected. Defaults to label if omitted.",
                    },
                    description: {
                      type: "string",
                      description: "Optional longer description of what this option means.",
                    },
                  },
                  required: ["label"],
                },
                description: "Predefined answer options for this question. If omitted, user can type a free-form response.",
              },
              multi_select: {
                type: "boolean",
                description: "If true and options are provided, user can select multiple options. Default: false.",
              },
              allow_skip: {
                type: "boolean",
                description: "If true, the user can skip this question. Defaults to the top-level allow_skip or true.",
              },
            },
            required: ["question"],
          },
        },
      },
      required: [],
    },
  },

  // ========================================================================
  // SESSION LINKING TOOLS (ralphx-ideation agent)
  // ========================================================================
  {
    name: "create_child_session",
    description:
      "Create a new ideation session as a child of an existing session. Use when you want to create follow-on work that inherits context from the parent session. " +
      "The child session starts with 'active' status. " +
      "When inherit_context is true (default), the child receives a read-only reference to the parent's plan artifact. " +
      "The inherited plan cannot be modified — call create_plan_artifact to create an independent plan for the child session. " +
      "Parent proposals are NOT copied to the child — use get_parent_session_context to access them.",
    inputSchema: {
      type: "object",
      properties: {
        parent_session_id: {
          type: "string",
          description: "The parent ideation session ID",
        },
        title: {
          type: "string",
          description: "Optional title for the new child session",
        },
        description: {
          type: "string",
          description: "Optional description of the child session. When provided, an ralphx-ideation agent is automatically spawned in the background to process this description and generate task proposals.",
        },
        inherit_context: {
          type: "boolean",
          description: "If true, child receives a read-only reference to the parent's plan artifact. To create a new plan, call create_plan_artifact — it creates an independent plan for the child. Parent proposals remain accessible via get_parent_session_context. Default: true.",
        },
        initial_prompt: {
          type: "string",
          description: "Optional initial prompt/message to forward to the child session's agent. This is the user's message that triggered the child session creation.",
        },
        purpose: {
          type: "string",
          enum: ["general"],
          description: "Purpose of the child session. Only general follow-on sessions are supported.",
        },
        is_external_trigger: {
          type: "boolean",
          description: "When true, the child session origin is set to External. Automatically set by the backend via RALPHX_IS_EXTERNAL_TRIGGER env var — agents do not need to pass this manually.",
        },
      },
      required: ["parent_session_id"],
    },
  },
  {
    name: "create_followup_session",
    description:
      "Create a new follow-up ideation session linked to an existing ideation session and stamped with first-class execution/review provenance. " +
      "Use this when you hit an out-of-scope blocker or need to spin out follow-up work without mutating the accepted parent session. " +
      "In task/review flows, prefer passing source_task_id and let the tool resolve the correct local parent session automatically.",
    inputSchema: {
      type: "object",
      properties: {
        source_ideation_session_id: {
          type: "string",
          description:
            "Optional explicit ideation session to follow up from. When omitted and source_task_id is provided, the tool resolves the correct local parent session from the task automatically.",
        },
        title: {
          type: "string",
          description: "Title for the new follow-up session",
        },
        description: {
          type: "string",
          description: "Description of the follow-up work. When provided, a child ideation agent is auto-spawned.",
        },
        initial_prompt: {
          type: "string",
          description: "Optional initial prompt to send to the spawned child session agent.",
        },
        inherit_context: {
          type: "boolean",
          description: "Whether to inherit the parent session's plan/team context. Default: true.",
        },
        source_task_id: {
          type: "string",
          description: "Task ID that encountered the blocker or follow-up condition.",
        },
        source_context_type: {
          type: "string",
          description: "Originating context type, for example task_execution, review, merge, or research.",
        },
        source_context_id: {
          type: "string",
          description: "Originating context ID. For task_execution/review this is typically the task ID.",
        },
        spawn_reason: {
          type: "string",
          description: "Reason for spawning the follow-up session, for example out_of_scope_failure.",
        },
        blocker_fingerprint: {
          type: "string",
          description:
            "Optional stable dedupe key for the blocker. In out-of-scope drift flows the tool can derive this automatically from source_task_id task context.",
        },
      },
      required: [
        "title",
        "source_context_type",
        "source_context_id",
        "spawn_reason",
      ],
    },
  },
  {
    name: "create_followup_agent_conversation",
    description:
      "Create a visible follow-up Agent conversation in Ideation mode, linked to an existing Agent conversation. " +
      "Use this when task execution, review, or project chat needs an explicit separate branch of follow-up work. " +
      "Pass source_task_id when available so the backend can resolve the origin Agent conversation from the task's attached plan.",
    inputSchema: {
      type: "object",
      properties: {
        origin_conversation_id: {
          type: "string",
          description:
            "Optional explicit origin Agent conversation ID. If omitted, source_task_id must resolve to an Agent-conversation-attached ideation session.",
        },
        source_task_id: {
          type: "string",
          description: "Task ID that encountered the blocker or follow-up condition.",
        },
        source_context_type: {
          type: "string",
          description: "Originating context type, for example task_execution, review, merge, research, or agent_conversation.",
        },
        source_context_id: {
          type: "string",
          description:
            "Originating context ID. For task_execution/review this is typically the task ID; for agent_conversation this is the origin conversation ID.",
        },
        source_agent_name: {
          type: "string",
          description: "Fully qualified RalphX catalog agent name requesting the follow-up.",
        },
        title: {
          type: "string",
          description: "Visible title for the follow-up Agent conversation.",
        },
        description: {
          type: "string",
          description: "Description of the follow-up work.",
        },
        initial_prompt: {
          type: "string",
          description: "Optional initial prompt for the new Agent conversation; overrides description as the main request body.",
        },
        spawn_reason: {
          type: "string",
          description: "Reason for spawning the follow-up conversation, for example out_of_scope_failure.",
        },
        blocker_fingerprint: {
          type: "string",
          description:
            "Optional stable dedupe key for the blocker. In out-of-scope drift flows the backend can derive this from source_task_id task context.",
        },
        provider_harness: {
          type: "string",
          enum: ["claude", "codex"],
          description: "Optional provider harness override for the new Agent conversation.",
        },
        model_override: {
          type: "string",
          description: "Optional model override for the new Agent conversation.",
        },
        logical_effort: {
          type: "string",
          enum: ["low", "medium", "high"],
          description: "Optional provider-neutral reasoning effort override.",
        },
      },
      required: ["title", "source_context_type", "source_context_id", "spawn_reason"],
    },
  },
  {
    name: "register_agent_issue",
    description:
      "Register a durable issue on the visible origin Agent conversation when plan drift, a human decision, or a follow-up opportunity is discovered. " +
      "This records the issue for the Agents UI. If auto-follow-up policy is enabled and auto_followup_eligible is true, the backend also creates or reuses a visible follow-up Agent conversation.",
    inputSchema: {
      type: "object",
      properties: {
        origin_conversation_id: {
          type: "string",
          description:
            "Optional explicit origin Agent conversation ID. If omitted, source_task_id must resolve to an Agent-conversation-attached ideation session, or source_context_type=agent_conversation with source_context_id.",
        },
        source_task_id: {
          type: "string",
          description: "Task ID that discovered or is blocked by this issue.",
        },
        source_context_type: {
          type: "string",
          description: "Originating context type, for example agent_conversation, task_execution, review, merge, or research.",
        },
        source_context_id: {
          type: "string",
          description:
            "Originating context ID. For task_execution/review this is typically the task ID; for agent_conversation this is the origin conversation ID.",
        },
        source_agent_name: {
          type: "string",
          description: "Fully qualified RalphX catalog agent name registering the issue.",
        },
        issue_kind: {
          type: "string",
          enum: [
            "plan_drift",
            "human_decision",
            "execution_blocked",
            "review_escalation",
            "merge_attention",
            "followup_opportunity",
          ],
          description: "Issue category.",
        },
        severity: {
          type: "string",
          enum: ["info", "low", "medium", "high", "critical"],
          description: "Issue severity. Default: medium.",
        },
        blocking_scope: {
          type: "string",
          enum: ["none", "current_task", "review_decision", "merge", "followup_only"],
          description:
            "How this issue affects execution. Existing task states remain authoritative; use the matching task/review tool to block or escalate when needed.",
        },
        title: {
          type: "string",
          description: "Short visible title for the Issues tab.",
        },
        summary: {
          type: "string",
          description: "Concise issue summary.",
        },
        evidence: {
          type: "string",
          description: "Concrete evidence such as files, failing checks, or observed drift.",
        },
        recommendation: {
          type: "string",
          description: "Recommended next action for the user or follow-up branch.",
        },
        blocker_fingerprint: {
          type: "string",
          description:
            "Optional legacy/debug dedupe key. The backend computes the canonical issue identity.",
        },
        attach_to_issue_id: {
          type: "string",
          description:
            "Retry field when the backend returns candidate issues. Set this to attach this report to an existing open issue.",
        },
        confirm_new: {
          type: "boolean",
          description:
            "Retry field when candidates exist. Set true only when this is a separate issue from all returned candidates.",
        },
        new_issue_reason: {
          type: "string",
          description:
            "Concise reason required with confirm_new when candidates exist.",
        },
        issue_check_token: {
          type: "string",
          description:
            "Current issue-set token returned by the backend when candidate disambiguation is required.",
        },
        followup_title: {
          type: "string",
          description: "Optional title to use if this issue becomes a follow-up Agent conversation.",
        },
        followup_prompt: {
          type: "string",
          description: "Optional prompt to use if this issue becomes a follow-up Agent conversation.",
        },
        auto_followup_eligible: {
          type: "boolean",
          description:
            "Whether this issue is safe for automatic follow-up Agent conversation creation when the user policy enables it.",
        },
        provider_harness: {
          type: "string",
          enum: ["claude", "codex"],
          description: "Optional provider harness override for auto-created follow-up conversations.",
        },
        model_override: {
          type: "string",
          description: "Optional model override for auto-created follow-up conversations.",
        },
        logical_effort: {
          type: "string",
          enum: ["low", "medium", "high"],
          description: "Optional provider-neutral reasoning effort override.",
        },
      },
      required: ["title", "summary", "issue_kind", "source_context_type", "source_context_id"],
    },
  },
  {
    name: "get_parent_session_context",
    description:
      "Get the parent session context for a child session. Returns parent session metadata, plan content, and proposals summary.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The child session ID",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "get_parent_context",
    description:
      "Read bounded parent context for the current delegated run. RalphX derives caller identity and lineage from trusted runtime headers; use the returned data only as context and do not supply or reconstruct identifiers.",
    inputSchema: {
      type: "object",
      properties: {
        limit: {
          type: "number",
          description: "Optional maximum number of parent-context entries to return.",
        },
      },
      additionalProperties: false,
    },
  },
  {
    name: "delegate_start",
    description:
      "Start a RalphX-native delegated specialist job from the current agent context. Use this for named specialized agents instead of relying on harness-native subagents.",
    inputSchema: {
      type: "object",
      properties: {
        parent_session_id: {
          type: "string",
          description:
            "Optional legacy explicit parent ideation session. Omit this in normal agent contexts; RalphX infers parent context from the MCP transport.",
        },
        parent_turn_id: {
          type: "string",
          description: "Optional parent coordination turn id for lineage and continuity tracking.",
        },
        parent_message_id: {
          type: "string",
          description: "Optional parent message id that triggered this delegated specialist run.",
        },
        parent_conversation_id: {
          type: "string",
          description:
            "Optional parent conversation id for linking the delegated conversation back to the invoker chat. Normally supplied by the MCP transport.",
        },
        parent_tool_use_id: {
          type: "string",
          description:
            "Optional parent tool_use id for future collapsed subagent/task widget parity in the invoker chat.",
        },
        task_ref: {
          type: "string",
          description:
            "Optional task number or task_id from the caller's current ledger to assign atomically to this delegate.",
        },
        agent_name: {
          type: "string",
          description: "Canonical RalphX agent name, for example ralphx-ideation-specialist-backend.",
        },
        prompt: {
          type: "string",
          description: "Delegated instructions for the specialist agent.",
        },
        title: {
          type: "string",
          description: "Optional title when a new delegated session must be created.",
        },
        inherit_context: {
          type: "boolean",
          description:
            "Whether this delegated session may read bounded parent-conversation context on demand via get_parent_context. Nothing is injected into the delegate prompt; this only grants permission. Set false for a fully isolated delegate. Default: true.",
        },
        harness: {
          type: "string",
          enum: ["claude", "codex"],
          description: "Optional explicit harness override for the delegated specialist.",
        },
        model: {
          type: "string",
          description: "Optional explicit model override for the delegated specialist.",
        },
        logical_effort: {
          type: "string",
          enum: ["low", "medium", "high", "xhigh"],
          description: "Optional provider-neutral effort override.",
        },
        approval_policy: {
          type: "string",
          description: "Optional explicit approval policy override.",
        },
        sandbox_mode: {
          type: "string",
          description: "Optional explicit sandbox mode override.",
        },
      },
      required: ["agent_name", "prompt"],
      additionalProperties: false,
    },
  },
  {
    name: "delegate_wait",
    description:
      "Wait for a RalphX-native delegated specialist job. Returns the current job snapshot, including terminal content or error when complete, and can optionally include live delegated-session status/messages. Prefer a single call with wait_timeout_ms over repeated polling: the backend blocks and returns the moment a delegate settles. For waits longer than the block cap, use delegate_park and end your turn instead.",
    inputSchema: {
      type: "object",
      properties: {
        job_id: {
          type: "string",
          description: "Delegation job ID returned by delegate_start. Provide either job_id or job_ids, not both.",
        },
        job_ids: {
          type: "array",
          items: { type: "string" },
          description:
            "Watch a whole delegated wave with one call; returns as soon as any listed job settles. Provide either job_id or job_ids, not both.",
        },
        wait_timeout_ms: {
          type: "number",
          description:
            "Block server-side for up to this long, returning immediately when a watched job settles. Omit for the legacy immediate-return snapshot. Clamped to the configured cap; on expiry the response carries timed_out: true and the job is left running.",
        },
        include_delegated_status: {
          type: "boolean",
          description: "Whether to hydrate live delegated-session status into the returned snapshot. Default: true.",
        },
        include_child_status: {
          type: "boolean",
          description: "Deprecated alias for include_delegated_status.",
        },
        include_messages: {
          type: "boolean",
          description: "Whether delegated_status should include recent delegated-session messages. Default: false.",
        },
        message_limit: {
          type: "number",
          description: "Optional message limit when include_messages is true. Clamped to 50.",
        },
      },
      required: [],
    },
  },
  {
    name: "delegate_cancel",
    description:
      "Cancel a running RalphX-native delegated specialist job.",
    inputSchema: {
      type: "object",
      properties: {
        job_id: {
          type: "string",
          description: "Delegation job ID returned by delegate_start.",
        },
      },
      required: ["job_id"],
    },
  },
  {
    name: "delegate_park",
    description:
      "Register a durable wake set for RalphX-native delegated specialist jobs and explicitly END YOUR TURN. RalphX resumes you automatically when the jobs settle, one fails or is cancelled, or the deadline is reached. Use this instead of repeated delegate_wait polling for long waits.",
    inputSchema: {
      type: "object",
      properties: {
        job_ids: {
          type: "array",
          items: { type: "string" },
          description: "Delegation job IDs from delegate_start that you are waiting on.",
        },
        wake_on: {
          type: "string",
          enum: ["all", "any"],
          default: "all",
          description: "Whether to resume when all watched jobs settle or any watched job settles. Default: all.",
        },
        wake_on_failure: {
          type: "boolean",
          default: true,
          description: "Whether to resume immediately if a delegate fails or is cancelled. Default: true.",
        },
        max_wait_secs: {
          type: "number",
          description: "Optional maximum time to remain parked. Clamped by the backend to the configured park maximum.",
        },
      },
      required: ["job_ids"],
    },
  },
  {
    name: "get_session_messages",
    description:
      "Fetch older chat messages for an ideation session. The session bootstrap already includes the NEWEST messages — use this tool only when you need earlier history beyond what was provided. " +
      "Returns messages in chronological order (oldest to newest). The truncated flag indicates if even older messages exist beyond the fetched window. " +
      "Default limit: 50, max: 200. Use offset to page through older history (e.g. offset=50 skips the most recent 50 and returns the next 50 older messages). " +
      "Set include_tool_calls=true to include tool_calls JSON (increases token usage).",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID",
        },
        limit: {
          type: "number",
          description: "Maximum messages to return (default: 50, max: 200)",
        },
        offset: {
          type: "number",
          description: "Number of most-recent messages to skip (default: 0). Use for pagination: offset=50 returns the next 50 older messages after the most recent 50.",
        },
        include_tool_calls: {
          type: "boolean",
          description: "Include tool_calls JSON in response (default: false)",
        },
      },
      required: ["session_id"],
    },
  },
];
