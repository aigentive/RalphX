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

export type AgentSidebarInboxFilter = "recent" | "stale" | "done";

export type AgentSidebarInboxFilterDescriptor = Readonly<{
  filter: AgentSidebarInboxFilter;
  label: string;
  emptyLabel: string;
}>;

export type AgeEscalationTone = "normal" | "warn" | "alert";

export type AgeEscalation = Readonly<{
  label: string;
  tone: AgeEscalationTone;
}>;

// The inbox's top-level filters. Recent has no single backing lane: it renders
// the `needs` and `working` lane queries as two groups in one scroller, so the
// backend still serves the same four attention lanes.
export const AGENT_SIDEBAR_INBOX_FILTERS = [
  { filter: "recent", label: "Recent", emptyLabel: "Nothing recent" },
  { filter: "stale", label: "Stale", emptyLabel: "Nothing stale" },
  { filter: "done", label: "Done", emptyLabel: "Nothing done" },
] as const satisfies readonly AgentSidebarInboxFilterDescriptor[];

export const AGENT_SIDEBAR_RECENT_GROUPS = [
  { lane: "needs", label: "Needs you", emptyLabel: "Nothing needs you" },
  { lane: "working", label: "Working", emptyLabel: "Nothing working" },
] as const satisfies readonly AgentSidebarInboxLaneDescriptor[];

export function laneForInboxFilter(
  filter: AgentSidebarInboxFilter,
): AgentSidebarAttentionLane | null {
  return filter === "recent" ? null : filter;
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
// have to stay legible in a 268px row: four uncapped totals wrap the labels.
// The exact number stays in the chip tooltip and accessible name.
export function formatInboxLaneCount(count: number): string {
  return count > INBOX_LANE_COUNT_CAP ? `${INBOX_LANE_COUNT_CAP}+` : `${count}`;
}

export function describeInboxLaneCount(label: string, count: number): string {
  return `${label}, ${count} ${count === 1 ? "conversation" : "conversations"}`;
}

export function shouldEscalateAge(lane: AgentSidebarAttentionLane): boolean {
  return lane !== "working" && lane !== "done";
}

export function formatParkedDelegateMeta(count: number): string | null {
  if (count <= 0) {
    return null;
  }
  return `Waiting on ${count} ${count === 1 ? "delegate" : "delegates"}`;
}

export function summarizeInboxLaneCounts(
  countsByLane: Readonly<Record<AgentSidebarAttentionLane, number>>
): { needsCount: number; footerLabel: string } {
  const needsCount = countsByLane.needs;
  if (needsCount === 0) {
    return { needsCount, footerLabel: "Nothing waiting on you" };
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
