import { describe, expect, it } from "vitest";

import {
  AGENT_SIDEBAR_INBOX_FILTERS,
  AGENT_SIDEBAR_RECENT_GROUPS,
  describeInboxLaneCount,
  formatInboxLaneCount,
  formatParkedDelegateMeta,
  getAgeEscalation,
  laneForInboxFilter,
  shouldEscalateAge,
  summarizeInboxLaneCounts,
} from "./agentSidebarInboxLanes";

const NOW = new Date("2026-07-28T12:00:00.000Z");
const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;

describe("AGENT_SIDEBAR_INBOX_FILTERS", () => {
  it("uses Recent, Stale, and Done as the top-level inbox filters", () => {
    expect(AGENT_SIDEBAR_INBOX_FILTERS).toEqual([
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
    ]);
  });
});

describe("AGENT_SIDEBAR_RECENT_GROUPS", () => {
  it("keeps Needs you and Working together in recency order", () => {
    expect(AGENT_SIDEBAR_RECENT_GROUPS).toEqual([
      { lane: "needs", label: "Needs you", emptyLabel: "Nothing needs you" },
      { lane: "working", label: "Working", emptyLabel: "Nothing running" },
    ]);
  });
});

describe("laneForInboxFilter", () => {
  it("uses a single backing lane only for stale and done filters", () => {
    expect(laneForInboxFilter("recent")).toBeNull();
    expect(laneForInboxFilter("stale")).toBe("stale");
    expect(laneForInboxFilter("done")).toBe("done");
  });
});

describe("formatInboxLaneCount", () => {
  it("renders exact counts up to the cap", () => {
    expect(formatInboxLaneCount(0)).toBe("0");
    expect(formatInboxLaneCount(99)).toBe("99");
  });

  it("caps counts above 99 so lane labels keep their width", () => {
    expect(formatInboxLaneCount(100)).toBe("99+");
    expect(formatInboxLaneCount(486)).toBe("99+");
  });
});

describe("describeInboxLaneCount", () => {
  it("keeps the exact count in the accessible name past the cap", () => {
    expect(describeInboxLaneCount("Done", 486)).toBe("Done, 486 conversations");
  });

  it("uses the singular noun for one conversation", () => {
    expect(describeInboxLaneCount("Needs you", 1)).toBe(
      "Needs you, 1 conversation",
    );
  });
});

describe("getAgeEscalation", () => {
  it("uses normal tone just below two days", () => {
    expect(getAgeEscalation(atAge(2 * DAY_MS - 1), NOW)).toEqual({
      label: "1d",
      tone: "normal",
    });
  });

  it("uses warn tone at two days", () => {
    expect(getAgeEscalation(atAge(2 * DAY_MS), NOW)).toEqual({
      label: "2d",
      tone: "warn",
    });
  });

  it("uses warn tone just above two days", () => {
    expect(getAgeEscalation(atAge(2 * DAY_MS + 1), NOW)).toEqual({
      label: "2d",
      tone: "warn",
    });
  });

  it("keeps warn tone just below seven days", () => {
    expect(getAgeEscalation(atAge(7 * DAY_MS - 1), NOW)).toEqual({
      label: "6d",
      tone: "warn",
    });
  });

  it("uses alert tone at seven days", () => {
    expect(getAgeEscalation(atAge(7 * DAY_MS), NOW)).toEqual({
      label: "1w",
      tone: "alert",
    });
  });

  it("uses alert tone just above seven days", () => {
    expect(getAgeEscalation(atAge(7 * DAY_MS + 1), NOW)).toEqual({
      label: "1w",
      tone: "alert",
    });
  });

  it("returns an empty normal value for an invalid timestamp", () => {
    expect(getAgeEscalation("", NOW)).toEqual({ label: "", tone: "normal" });
    expect(getAgeEscalation("not-a-timestamp", NOW)).toEqual({
      label: "",
      tone: "normal",
    });
  });
});

describe("shouldEscalateAge", () => {
  it("keeps working parked coordinators out of stale age escalation", () => {
    expect(shouldEscalateAge("needs")).toBe(true);
    expect(shouldEscalateAge("stale")).toBe(true);
    expect(shouldEscalateAge("working")).toBe(false);
    expect(shouldEscalateAge("done")).toBe(false);
  });
});

describe("formatParkedDelegateMeta", () => {
  it("uses singular and plural delegate copy for parked coordinators", () => {
    expect(formatParkedDelegateMeta(1)).toBe("Waiting on 1 delegate");
    expect(formatParkedDelegateMeta(2)).toBe("Waiting on 2 delegates");
  });

  it("omits parked delegate copy when no delegates remain", () => {
    expect(formatParkedDelegateMeta(0)).toBeNull();
  });
});

describe("summarizeInboxLaneCounts", () => {
  it("uses the empty footer when no conversations need attention", () => {
    expect(
      summarizeInboxLaneCounts({ needs: 0, working: 4, stale: 2, done: 1 })
    ).toEqual({ needsCount: 0, footerLabel: "Nothing waiting on you" });
  });

  it("uses the singular footer for one conversation", () => {
    expect(
      summarizeInboxLaneCounts({ needs: 1, working: 0, stale: 0, done: 0 })
    ).toEqual({ needsCount: 1, footerLabel: "1 waiting on you" });
  });

  it("uses the plural footer for multiple conversations", () => {
    expect(
      summarizeInboxLaneCounts({ needs: 3, working: 0, stale: 0, done: 0 })
    ).toEqual({ needsCount: 3, footerLabel: "3 waiting on you" });
  });
});

function atAge(ageMs: number): string {
  return new Date(NOW.getTime() - ageMs).toISOString();
}
