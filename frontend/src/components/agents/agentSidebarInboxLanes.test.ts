import { describe, expect, it } from "vitest";

import {
  AGENT_SIDEBAR_INBOX_LANES,
  getAgeEscalation,
  shouldEscalateAge,
  summarizeInboxLaneCounts,
} from "./agentSidebarInboxLanes";

const NOW = new Date("2026-07-28T12:00:00.000Z");
const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;

describe("AGENT_SIDEBAR_INBOX_LANES", () => {
  it("keeps the inbox lanes in attention order with their labels", () => {
    expect(AGENT_SIDEBAR_INBOX_LANES).toEqual([
      { lane: "needs", label: "Needs you", emptyLabel: "Nothing needs you" },
      { lane: "working", label: "Working", emptyLabel: "Nothing working" },
      { lane: "stale", label: "Stale", emptyLabel: "Nothing stale" },
      { lane: "done", label: "Done", emptyLabel: "Nothing done" },
    ]);
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
  it("exempts working and done lanes from age escalation", () => {
    expect(shouldEscalateAge("needs")).toBe(true);
    expect(shouldEscalateAge("stale")).toBe(true);
    expect(shouldEscalateAge("working")).toBe(false);
    expect(shouldEscalateAge("done")).toBe(false);
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
