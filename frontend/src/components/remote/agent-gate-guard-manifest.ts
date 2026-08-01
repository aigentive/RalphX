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
 * The fork's other eight files (`CompletedTaskDetail`, `ExecutionTaskDetail`,
 * `MergedTaskDetail`, `MergePhaseTimeline`, `MergingTaskDetail`, `ReviewingTaskDetail`,
 * `RevisionTaskDetail`, `WaitingTaskDetail`) carry no steering affordance today — their
 * buttons navigate, expand, or copy. Phase 4 must re-check them when it ports the gate
 * wiring, and add them here if any of them grows a mutating control.
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
  "src/hooks/useIdeation.ts",
  // The Agents-pane fork (Phase 0 work item 3) — all five currently ungated.
  "src/components/agents/task-details/detail-views/BasicTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/EscalatedTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/HumanReviewTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/MergeConflictTaskDetail.tsx",
  "src/components/agents/task-details/detail-views/MergeIncompleteTaskDetail.tsx",
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
  // --- dead affordance rows (no production surface resolves them) ------------
  {
    kind: "dead-row",
    id: "dead-row:automationRunNow",
    owner: 4,
    why: "Run now is gated by `automationResume` instead, so the unregistered trigger_automation_run_now renders as an enabled button (confirmed critical).",
  },
  {
    kind: "dead-row",
    id: "dead-row:automationRestart",
    owner: 4,
    why: "Restart is gated by `automationResume` in the Agents panel and ungated on the Automations page.",
  },
  {
    kind: "dead-row",
    id: "dead-row:folderReferenceRemove",
    owner: 4,
    why: "The folder-reference chip's remove button is ungated while its `folderReferenceAdd` sibling four lines away is gated.",
  },
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
  {
    kind: "op-mismatch",
    id: "op-mismatch:src/components/PermissionDialog.tsx::permissionApprove",
    owner: 1,
    why: "Gate resolves the pinned approve_permission_request while the dialog invokes the raw resolve_permission_request twin.",
  },
  {
    kind: "op-mismatch",
    id: "op-mismatch:src/components/Ideation/PlanEditor.tsx::artifactEdit",
    owner: 4,
    why: "Gate resolves update_artifact; the save path POSTs update_plan_artifact, which is not on the remount allowlist (confirmed critical).",
  },

  // --- raw twins still invoked ---------------------------------------------
  {
    kind: "raw-twin",
    id: "raw-twin:src/api/permission.ts::resolve_permission_request",
    owner: 1,
    why: "Approve/deny must route to the pinned approve_/deny_permission_request ops under a remote environment.",
  },
  {
    kind: "raw-twin",
    id: "raw-twin:src/api/ask-user-question.ts::resolve_user_question",
    owner: 1,
    why: "The requestId branch must route to the registered answer_user_question remotely; resolve_user_question is unreachable at every scope.",
  },

  // --- ungated wiring (the Agents-pane detail-view fork) ---------------------
  {
    kind: "wiring",
    id: "wiring:src/components/agents/task-details/detail-views/BasicTaskDetail.tsx",
    owner: 4,
    why: "Fork of the gated twin; unblock/retry controls render enabled remotely.",
  },
  {
    kind: "wiring",
    id: "wiring:src/components/agents/task-details/detail-views/EscalatedTaskDetail.tsx",
    owner: 4,
    why: "Fork of the gated twin; approve / request-changes render enabled remotely.",
  },
  {
    kind: "wiring",
    id: "wiring:src/components/agents/task-details/detail-views/HumanReviewTaskDetail.tsx",
    owner: 4,
    why: "Fork of the gated twin; approve / request-changes render enabled remotely.",
  },
  {
    kind: "wiring",
    id: "wiring:src/components/agents/task-details/detail-views/MergeConflictTaskDetail.tsx",
    owner: 4,
    why: "retry_merge / resolve_merge_conflict have no affordance row in either copy and no gate here.",
  },
  {
    kind: "wiring",
    id: "wiring:src/components/agents/task-details/detail-views/MergeIncompleteTaskDetail.tsx",
    owner: 4,
    why: "retry_merge / resolve_merge_conflict have no affordance row in either copy and no gate here.",
  },
];

/** The quarantined ids of one kind, as a set — the guards diff against these. */
export function quarantinedIds(kind: GateGapKind): ReadonlySet<string> {
  return new Set(
    KNOWN_GATE_GAPS.filter((gap) => gap.kind === kind).map((gap) => gap.id)
  );
}
