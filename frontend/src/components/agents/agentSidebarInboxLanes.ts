import type { AgentSidebarAttentionLane } from "@/api/chat";

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const WEEK_MS = 7 * DAY_MS;
const WARN_AGE_MS = 2 * DAY_MS;
const ALERT_AGE_MS = 7 * DAY_MS;

export type AgentSidebarInboxLaneDescriptor = Readonly<{
  lane: AgentSidebarAttentionLane;
  label: string;
  emptyLabel: string;
}>;

export type AgentSidebarInboxFilter = "recent" | "reviews" | "stale" | "done";

// Filters that render several lane queries in one scroller, so they have no
// single backing lane key.
const COMPOSITE_INBOX_FILTERS = ["recent", "reviews"] as const;

export type AgentSidebarInboxEmptyState = Readonly<{
  headline: string;
  subline: string;
  tone: "win" | "calm";
}>;

export type AgentSidebarInboxFilterDescriptor = Readonly<{
  filter: AgentSidebarInboxFilter;
  label: string;
  emptyState: AgentSidebarInboxEmptyState;
}>;

export type AgeEscalationTone = "normal" | "warn" | "alert";

export type AgeEscalation = Readonly<{
  label: string;
  tone: AgeEscalationTone;
}>;

// The inbox's top-level filters. Recent and PR Reviews have no single backing
// lane: each renders several lane queries as groups in one scroller, so the
// backend still serves the same seven attention lanes.
//
// A filter's `filter` key and its display `label` are deliberately different.
// The key becomes the scroll key, chip id, `data-testid`, ARIA ids, and the
// persisted `sidebarInboxActiveLane` value, so renaming a label must never
// invalidate a persisted lane selection or a test id.
export const AGENT_SIDEBAR_INBOX_FILTERS = [
  {
    filter: "recent",
    label: "Recent",
    emptyState: {
      headline: "Inbox zero",
      subline: "Nothing needs you, nothing is running. Good moment to start the next thing.",
      tone: "win",
    },
  },
  {
    filter: "reviews",
    label: "PR Reviews",
    emptyState: {
      headline: "No open reviews",
      subline: "Start an agent in Review PR mode to track a pull request here.",
      tone: "calm",
    },
  },
  {
    filter: "stale",
    label: "Stale",
    emptyState: {
      headline: "Nothing has gone stale",
      subline: "Threads move here after two days without activity. Nothing is drifting.",
      tone: "calm",
    },
  },
  {
    filter: "done",
    label: "Done",
    emptyState: {
      headline: "Nothing finished yet",
      subline: "Merged and closed conversations collect here once work lands.",
      tone: "calm",
    },
  },
] as const satisfies readonly AgentSidebarInboxFilterDescriptor[];

export const AGENT_SIDEBAR_INBOX_FILTERED_EMPTY: AgentSidebarInboxEmptyState = {
  headline: "No matches",
  subline: "No conversations match the current search and filters.",
  tone: "calm",
};

export const AGENT_SIDEBAR_RECENT_GROUPS = [
  { lane: "needs", label: "Needs you", emptyLabel: "Nothing needs you" },
  { lane: "working", label: "Working", emptyLabel: "Nothing running" },
] as const satisfies readonly AgentSidebarInboxLaneDescriptor[];

// Watching is the resting classification this lane exists for: the review is
// finished on your side but the pull request is still live on GitHub.
export const AGENT_SIDEBAR_REVIEW_GROUPS = [
  { lane: "review_needs", label: "Needs you", emptyLabel: "No reviews need you" },
  { lane: "review_working", label: "Working", emptyLabel: "No reviews running" },
  { lane: "review_watching", label: "Watching", emptyLabel: "Nothing on GitHub" },
] as const satisfies readonly AgentSidebarInboxLaneDescriptor[];

export function laneForInboxFilter(
  filter: AgentSidebarInboxFilter,
): AgentSidebarAttentionLane | null {
  return (COMPOSITE_INBOX_FILTERS as readonly string[]).includes(filter)
    ? null
    : (filter as Exclude<AgentSidebarInboxFilter, (typeof COMPOSITE_INBOX_FILTERS)[number]>);
}

// The lane keys a composite filter sums and renders. Keeping this beside the
// descriptors means a new composite filter cannot be added without declaring
// its lanes.
export function lanesForInboxFilter(
  filter: AgentSidebarInboxFilter,
): readonly AgentSidebarAttentionLane[] {
  if (filter === "recent") {
    return AGENT_SIDEBAR_RECENT_GROUPS.map((group) => group.lane);
  }
  if (filter === "reviews") {
    return AGENT_SIDEBAR_REVIEW_GROUPS.map((group) => group.lane);
  }
  return [filter];
}

const REVIEW_STATE_LABELS: Readonly<Record<string, string>> = {
  reviewing: "Reviewing",
  submitting: "Submitting",
  needs_approval: "Needs approval",
  needs_decision_changes: "Approve request changes",
  needs_decision_comment: "Approve comment",
  needs_decision: "Decision needed",
  head_moved: "Head moved",
  blocked: "Blocked",
  approved: "Approved",
  changes_requested: "Changes requested",
  commented: "Commented",
  watching: "Watching",
  paused: "Paused",
};

export type ReviewStateTone =
  | "accent"
  | "warning"
  | "error"
  | "success"
  | "info"
  | "muted";

const REVIEW_STATE_TONES: Readonly<Record<string, ReviewStateTone>> = {
  reviewing: "accent",
  submitting: "accent",
  needs_approval: "warning",
  needs_decision_changes: "warning",
  needs_decision_comment: "warning",
  needs_decision: "warning",
  head_moved: "warning",
  blocked: "error",
  approved: "success",
  changes_requested: "info",
  commented: "info",
  watching: "muted",
  paused: "muted",
};

// An unrecognized backend key degrades to the generic resting copy rather
// than rendering a raw snake_case string in the row meta line.
export function reviewStateLabel(reviewState: string): string {
  return REVIEW_STATE_LABELS[reviewState] ?? REVIEW_STATE_LABELS.watching!;
}

export function reviewStateTone(reviewState: string): ReviewStateTone {
  return REVIEW_STATE_TONES[reviewState] ?? "muted";
}

export function getAgeEscalation(
  lastActivityIso: string,
  now: Date
): AgeEscalation {
  const lastActivity = new Date(lastActivityIso);
  const nowMs = now.getTime();
  const lastActivityMs = lastActivity.getTime();

  if (Number.isNaN(nowMs) || Number.isNaN(lastActivityMs)) {
    return { label: "", tone: "normal" };
  }

  const ageMs = Math.max(0, nowMs - lastActivityMs);
  return {
    label: formatCompactAge(ageMs),
    tone: getAgeEscalationTone(ageMs),
  };
}

const INBOX_LANE_COUNT_CAP = 99;

// Lane chip counts are the only cross-lane signal left in the switcher, so they
// have to stay legible in the narrowest sidebar: four uncapped totals wrap the
// labels. The exact number stays in the chip tooltip and accessible name.
export function formatInboxLaneCount(count: number): string {
  return count > INBOX_LANE_COUNT_CAP ? `${INBOX_LANE_COUNT_CAP}+` : `${count}`;
}

export function describeInboxLaneCount(label: string, count: number): string {
  return `${label}, ${count} ${count === 1 ? "conversation" : "conversations"}`;
}

// Working and resting lanes are calm by definition; only lanes that are
// actually waiting on the user age visibly. `review_watching` is deliberately
// included: a review that is done on your side and live on GitHub is not
// drifting, however long it sits there.
const NON_ESCALATING_LANES: readonly AgentSidebarAttentionLane[] = [
  "working",
  "done",
  "review_working",
  "review_watching",
];

export function shouldEscalateAge(lane: AgentSidebarAttentionLane): boolean {
  return !NON_ESCALATING_LANES.includes(lane);
}

export function formatParkedDelegateMeta(count: number): string | null {
  if (count <= 0) {
    return null;
  }
  return `Waiting on ${count} ${count === 1 ? "delegate" : "delegates"}`;
}

// The footer is the only cross-lane signal for reviews: the PR Reviews chip
// carries no attention dot, so reviews waiting on you are counted here.
export function summarizeInboxLaneCounts(
  countsByLane: Readonly<Record<AgentSidebarAttentionLane, number>>
): { needsCount: number; footerLabel: string } {
  const reviewNeedsCount = countsByLane.review_needs;
  const needsCount = countsByLane.needs + reviewNeedsCount;
  if (needsCount === 0) {
    return { needsCount, footerLabel: "Nothing waiting on you" };
  }

  // Naming the reviews explicitly when they are the only thing waiting points
  // at the chip the user would otherwise have to go looking for.
  if (countsByLane.needs === 0) {
    return {
      needsCount,
      footerLabel: `${reviewNeedsCount} ${
        reviewNeedsCount === 1 ? "review" : "reviews"
      } waiting on you`,
    };
  }

  return {
    needsCount,
    footerLabel: `${needsCount} waiting on you`,
  };
}

function getAgeEscalationTone(ageMs: number): AgeEscalationTone {
  if (ageMs >= ALERT_AGE_MS) {
    return "alert";
  }
  if (ageMs >= WARN_AGE_MS) {
    return "warn";
  }
  return "normal";
}

function formatCompactAge(ageMs: number): string {
  if (ageMs < HOUR_MS) {
    return `${Math.floor(ageMs / MINUTE_MS)}m`;
  }
  if (ageMs < DAY_MS) {
    return `${Math.floor(ageMs / HOUR_MS)}h`;
  }
  if (ageMs < WEEK_MS) {
    return `${Math.floor(ageMs / DAY_MS)}d`;
  }
  return `${Math.floor(ageMs / WEEK_MS)}w`;
}
