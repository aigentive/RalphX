/**
 * Shared manifest for the remote gate guards (Phase 0 of the remote-coverage handoff).
 *
 * ## Why this file exists
 *
 * The wiring guard in `agent-gate-surfaces.test.tsx` certified files as "correctly
 * gated" by proving only that they IMPORT `useAgentGate`. Two confirmed criticals lived
 * inside files that guard had certified: `AgentsAutomationPanel`'s "Run now" button
 * gated by `automationResume` (an op it never invokes), and `PlanEditor`'s gate
 * resolving `update_artifact` while its save path POSTs `update_plan_artifact`. A guard
 * that cannot see the op↔callsite relationship cannot catch either one.
 * Op reach is file-granular: once `automationRunNow` is consumed in
 * `AgentsAutomationPanel.tsx`, its legitimate `resume_automation` call can hide a same-file
 * wrong-op regression. Phase 4 must add a mounted behavioral test for Run now.
 *
 * `agent-gate-op-consistency.test.ts` adds that relationship. This module holds the
 * three hand-maintained lists both guards read, so the file list and the quarantine
 * live in exactly one place.
 *
 * ## The quarantine ratchet (KNOWN_GATE_GAPS)
 *
 * Phase 0 is guards-only: it must ship a GREEN suite while still recording every real
 * defect the later phases own. So each currently-failing entry is quarantined here, and
 * the guards assert BOTH directions:
 *
 *   1. every gap that is NOT quarantined fails the build (no new defects), and
 *   2. every quarantined gap STILL FAILS (no stale quarantine).
 *
 * Direction 2 is the ratchet and matters more than the guard itself: fixing a defect
 * without deleting its row turns the suite red, so the list can only shrink.
 *
 * ## How to shrink this list (do this in Phases 1/2/4)
 *
 *   1. Fix the underlying defect (reroute the command, consume the affordance row, wire
 *      the gate into the file).
 *   2. Run `npx vitest run src/components/remote` — the ratchet test for that entry now
 *      fails with "no longer failing".
 *   3. DELETE the entry from `KNOWN_GATE_GAPS`. Never edit `why` to describe the fix,
 *      never move it to the indirection allowlist to silence it, and never add an entry
 *      for a defect you are introducing — the list is append-never.
 *   4. Re-run; the suite is green with one fewer gap.
 *
 * An entry is only correctly reclassified into `GATE_CALLSITE_INDIRECTIONS` if the guard
 * was WRONG about it (the dispatch is real but invisible to static analysis), and that
 * reclassification needs the same one-line reason every other allowlist row carries.
 */

import type { AgentGatedAffordance } from "@/lib/remote/agent-gate";

// ---------------------------------------------------------------------------
// Wiring guard file list
// ---------------------------------------------------------------------------

/**
 * Files that must consult `useAgentGate` / `useFieldGate`.
 *
 * The `components/agents/task-details/detail-views/*` block is the fork the Agents pane
 * actually renders — the ungated twin of `components/tasks/detail-views/*`. It was
 * absent from this list, which is how an entire rendered surface shipped with no remote
 * gating at all. Every one of those five entries is quarantined below as an expected
 * failure and is Phase 4's worklist.
 *
 * The non-mutating remainder is `MergedTaskDetail` (opens the PR URL only),
 * `MergePhaseTimeline`, `RevisionTaskDetail`, and `WaitingTaskDetail`; they only present,
 * navigate, expand, or copy state, so they are excluded until they gain a mutation.
 */
export const GATE_WIRED_FILES: readonly string[] = [
  "src/components/agents/AgentComposerSurface.tsx",
  "src/components/Chat/ChatInput.tsx",
  "src/components/PermissionDialog.tsx",
  "src/hooks/useQuestionInput.ts",
  "src/components/tasks/TaskBoard/TaskBoard.tsx",
  "src/components/tasks/TaskBoard/Column.tsx",
  "src/components/tasks/TaskContextMenuItems.tsx",
  "src/components/tasks/GroupContextMenuItems.tsx",
  "src/components/tasks/detail-views/HumanReviewTaskDetail.tsx",
  "src/components/tasks/detail-views/EscalatedTaskDetail.tsx",
  "src/components/tasks/detail-views/BasicTaskDetail.tsx",
  "src/components/tasks/TaskEditForm.tsx",
  "src/components/agents/task-details/TaskEditForm.tsx",
  "src/components/agents/task-details/StepList.tsx",
  "src/components/Ideation/PlanEditor.tsx",
  "src/components/Ideation/ProposalCard.tsx",
  "src/components/Ideation/ProposalDetailSheet.tsx",
  "src/components/agents/AgentsAutomationPanel.tsx",
  "src/components/automations/AutomationDetailView.tsx",
  "src/components/automations/AutomationDetailHeader.tsx",
  "src/components/automations/AutomationRunsTab.tsx",
  "src/components/automations/AutomationRunTimelineItem.tsx",
  "src/hooks/useIdeation.ts",
  // The Agents-pane fork (Phase 0 work item 3) — all five currently ungated.
  "src/components/agents/task-details/detail-views/BasicTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/EscalatedTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/HumanReviewTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/MergeConflictTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/MergeIncompleteTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/ExecutionTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/MergingTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/ReviewingTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/CompletedTaskDetail.tsx",
  "src/components/tasks/detail-views/ExecutionTaskDetail.tsx",
  "src/components/tasks/detail-views/MergingTaskDetail.tsx",
  "src/components/tasks/detail-views/ReviewingTaskDetail.tsx",
  "src/components/tasks/detail-views/CompletedTaskDetail.tsx",
  "src/components/tasks/detail-views/MergeConflictTaskDetail.tsx",
  "src/components/tasks/detail-views/MergeIncompleteTaskDetail.tsx",
];

// ---------------------------------------------------------------------------
// Indirection allowlist
// ---------------------------------------------------------------------------

/**
 * `file :: affordance` pairs where the gated dispatch genuinely leaves the file through
 * an injected callback, so no static reader can see the command. These are NOT defects
 * and NOT quarantine entries — the guard is blind here, and says so.
 *
 * Every row needs a one-line reason naming the prop/param the dispatch escapes through.
 */
export const GATE_CALLSITE_INDIRECTIONS: readonly {
  readonly file: string;
  readonly affordance: AgentGatedAffordance;
  readonly reason: string;
}[] = [
  {
    file: "src/components/Chat/ChatInput.tsx",
    affordance: "chatSend",
    reason: "Presentational input; send dispatches through the `onSend` prop the host owns.",
  },
  {
    file: "src/components/agents/AgentsAutomationPanel.tsx",
    affordance: "automationSetupEdit",
    reason:
      "Setup edits post through `automationsApi.setupAgent.updateAutomation`, whose `update_automation` route name is computed inside `postAutomationJson` — invisible to the literal reach walk.",
  },
  {
    file: "src/components/Ideation/FinalizeConfirmationDialog.tsx",
    affordance: "ideationAcceptFinalize",
    reason:
      "Accept dispatches through `ideationApi.acceptFinalize`, whose route path is a template literal in `api/ideation.ts` — invisible to the literal reach walk.",
  },
  {
    file: "src/components/Ideation/FinalizeConfirmationDialog.tsx",
    affordance: "ideationRejectFinalize",
    reason:
      "Reject dispatches through `ideationApi.rejectFinalize`, whose route path is a template literal in `api/ideation.ts` — invisible to the literal reach walk.",
  },
  {
    file: "src/hooks/useIdeationEvents.ts",
    affordance: "ideationAcceptFinalize",
    reason:
      "The event-driven accept dispatches through `ideationApi.acceptFinalize`; the route path is a template literal in `api/ideation.ts` — invisible to the literal reach walk.",
  },
  {
    file: "src/components/agents/AgentComposerSurface.tsx",
    affordance: "agentComposerSend",
    reason: "Composer dispatches through the `onSend` prop; the invoke lives in the conversation host.",
  },
  {
    file: "src/components/agents/AgentComposerSurface.tsx",
    affordance: "agentStop",
    reason: "Stop dispatches through the optional `onStop` prop; the invoke lives in the conversation host.",
  },
  {
    file: "src/hooks/useQuestionInput.ts",
    affordance: "questionAnswer",
    reason: "Answer submission is the injected `submitAnswer` param (supplied by useAskUserQuestion).",
  },
  {
    file: "src/components/Chat/QueuedMessage.tsx",
    affordance: "queuedMessageDelete",
    reason: "Chip renders the gate; the delete invoke lives in useChatActions and arrives as the `onDelete` prop.",
  },
  {
    file: "src/components/Chat/QueuedMessage.tsx",
    affordance: "queuedMessageEdit",
    reason: "Chip renders the gate; the edit invoke lives in useChatActions and arrives as the `onEdit` prop.",
  },
  {
    file: "src/components/Chat/QueuedMessage.tsx",
    affordance: "queuedMessageSendNow",
    reason: "Chip renders the gate; the invoke lives in useChatActions and arrives as the `onSendNow` prop.",
  },
  {
    file: "src/components/Chat/ChatInput.tsx",
    affordance: "queuedMessageEdit",
    reason: "ArrowUp enters edit through `onEditLastQueued`; the delete invoke lives in useChatActions.",
  },
  {
    file: "src/components/tasks/TaskContextMenuItems.tsx",
    affordance: "taskMove",
    reason: "Menu items dispatch through the `handlers` prop map; no command is invoked in this file.",
  },
  {
    file: "src/components/tasks/TaskContextMenuItems.tsx",
    affordance: "taskApprove",
    reason: "Menu items dispatch through the `handlers` prop map; no command is invoked in this file.",
  },
  {
    file: "src/components/tasks/TaskContextMenuItems.tsx",
    affordance: "taskResume",
    reason: "Menu items dispatch through the `handlers` prop map; no command is invoked in this file.",
  },
  {
    file: "src/components/tasks/TaskContextMenuItems.tsx",
    affordance: "taskUnblock",
    reason: "Menu items dispatch through the `handlers` prop map; no command is invoked in this file.",
  },
  {
    file: "src/components/tasks/GroupContextMenuItems.tsx",
    affordance: "taskResume",
    reason: "Resume All dispatches through the `onResumeAll` prop supplied by the board.",
  },
  {
    file: "src/components/Ideation/ProposalCard.tsx",
    affordance: "proposalEdit",
    reason: "Edit/delete dispatch through the `onEdit` / `onDelete` props owned by the ideation host.",
  },
  {
    file: "src/components/Ideation/ProposalDetailSheet.tsx",
    affordance: "proposalEdit",
    reason: "Edit dispatches through the `onEdit` prop owned by the ideation host.",
  },
  ...([
    ["automationFinalize", "onApprove"],
    ["automationRestart", "onRestart"],
    ["automationResume", "onResume"],
    ["automationPause", "onPause"],
    ["automationRunNow", "onRunNow"],
    ["automationStop", "onCancelAutomation"],
    ["automationSkipJudge", "onSkipJudge"],
    ["automationSetupEdit", "onEdit"],
    ["automationDelete", "onDelete"],
  ] as const).map(([affordance, prop]) => ({
    file: "src/components/automations/AutomationDetailHeader.tsx",
    affordance,
    reason: `Header dispatches through the \`${prop}\` prop owned by AutomationDetailView.`,
  })),
  ...([
    ["automationRetryPlanJudge", "onRetryPlanJudge"],
    ["automationRetryJudge", "onRetryJudge"],
    ["automationRunNow", "onRunNow"],
  ] as const).map(([affordance, prop]) => ({
    file: "src/components/automations/AutomationRunsTab.tsx",
    affordance,
    reason: `Runs tab dispatches through the \`${prop}\` prop owned by AutomationDetailView.`,
  })),
  ...([
    ["automationDeleteRun", "onDeleteRun"],
    ["automationResumeRun", "onResumeRun"],
  ] as const).map(([affordance, prop]) => ({
    file: "src/components/automations/AutomationRunTimelineItem.tsx",
    affordance,
    reason: `Timeline item dispatches through the \`${prop}\` prop owned by AutomationDetailView.`,
  })),
];

/**
 * Affordance rows consumed through a DIFFERENT gate entry point than
 * `useAgentGate("<row>")`, so the consumer scan cannot see the row name.
 *
 * One line of justification each; anything not listed here and not consumed is a dead
 * row and must be quarantined or deleted.
 */
export const AFFORDANCE_CONSUMPTION_ALIASES: readonly {
  readonly affordance: AgentGatedAffordance;
  readonly reason: string;
}[] = [
  {
    affordance: "taskEditContent",
    reason:
      "Argument-level row: both TaskEditForms consume it as useFieldGate(\"update_task\", \"title\"), which routes to the same escalation branch in resolveAffordanceGate.",
  },
];

// ---------------------------------------------------------------------------
// Quarantine
// ---------------------------------------------------------------------------

/** Which handoff phase owns the fix for a quarantined gap. */
export type GateGapOwnerPhase = 1 | 2 | 4;

export type GateGapKind =
  /** An `AGENT_GATED_AFFORDANCES` row no production surface resolves. */
  | "dead-row"
  /** A file resolves an affordance whose op it never reaches. */
  | "op-mismatch"
  /** A production `invoke(` of a facade-split raw command. */
  | "raw-twin"
  /** A file on `GATE_WIRED_FILES` that does not consult the gate hook. */
  | "wiring";

export interface KnownGateGap {
  readonly kind: GateGapKind;
  /** Stable identity the guards compute independently: see each guard's `gapId`. */
  readonly id: string;
  readonly owner: GateGapOwnerPhase;
  readonly why: string;
}

/**
 * Every gate defect that exists on this branch today. Green suite, honest ledger.
 *
 * Shrink procedure is documented at the top of this file. Do not add rows.
 */
export const KNOWN_GATE_GAPS: readonly KnownGateGap[] = [
  {
    kind: "dead-row",
    id: "dead-row:chatContinueIdle",
    owner: 4,
    why: "The idle-continuation half of remote send has no consumer, so a host predating WP1 still renders an enabled send instead of the unavailable hint.",
  },
  {
    kind: "dead-row",
    id: "dead-row:stepUpdate",
    owner: 4,
    why: "update_task_step is invoked from api/tasks.ts but no step-editing surface resolves the row.",
  },

  // --- op↔callsite mismatches ----------------------------------------------
  // RETIRED by the Phase 1 merge: `op-mismatch:src/components/PermissionDialog.tsx::permissionApprove`.
  // `api/permission.ts` now routes approve → `approve_permission_request` and deny →
  // `deny_permission_request` under a remote environment, so the gate and the callsite agree.
  // The ratchet turned red on the stale entry, which is exactly what it is for — the row is
  // deleted rather than the assertion relaxed.

  // --- raw twins still invoked ---------------------------------------------
  {
    kind: "raw-twin",
    id: "raw-twin:src/api/permission.ts::resolve_permission_request",
    owner: 1,
    why: "The local transport branch legitimately invokes resolve_permission_request; the remote branch routes to the pinned approve_/deny_permission_request twins.",
  },
  {
    kind: "raw-twin",
    id: "raw-twin:src/api/ask-user-question.ts::resolve_user_question",
    owner: 1,
    why: "The local transport branch legitimately invokes resolve_user_question; the remote branch uses the honest gate because this wire cannot signal the MCP long-poll.",
  },

];

export function orphanedWiringGapIds(
  gaps: readonly KnownGateGap[],
  wiredFiles: readonly string[]
): string[] {
  const wired = new Set(wiredFiles);
  return gaps
    .filter((gap) => gap.kind === "wiring")
    .filter((gap) => !wired.has(gap.id.slice("wiring:".length)))
    .map((gap) => gap.id);
}

/** The quarantined ids of one kind, as a set — the guards diff against these. */
export function quarantinedIds(kind: GateGapKind): ReadonlySet<string> {
  return new Set(
    KNOWN_GATE_GAPS.filter((gap) => gap.kind === kind).map((gap) => gap.id)
  );
}
