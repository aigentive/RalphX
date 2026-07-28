/**
 * The transition table is an ARTIFACT, so it gets its own falsification pass before any
 * controller runs it: totality, and the structural invariants §6.5 states in prose
 * (`blocked`/`suspended`/`offline` never schedule a retry, nothing but a completed
 * attempt reaches `connected`, the ladder clamps).
 *
 * `supervisor.test.ts` then drives the real controller from the same rows, so a row that
 * exists here but is not honoured there fails as well.
 */

import { describe, expect, it } from "vitest";

import {
  CONNECT_BUDGET_MS,
  RETRY_LADDER_MAX_MS,
  RETRY_LADDER_MS,
  STABLE_PERIOD_MS,
  SUPERVISOR_EVENTS,
  SUPERVISOR_STATES,
  SUPERVISOR_TRANSITIONS,
  SUPERVISOR_TRANSITION_TABLE,
  lookupTransition,
  presentationFor,
  retryDelayMs,
  type SupervisorEvent,
  type SupervisorState,
} from "./supervisor-transition-table";

describe("transition table totality", () => {
  it("defines every event for every state", () => {
    expect(SUPERVISOR_TRANSITIONS).toHaveLength(
      SUPERVISOR_STATES.length * SUPERVISOR_EVENTS.length
    );
    for (const state of SUPERVISOR_STATES) {
      for (const event of SUPERVISOR_EVENTS) {
        expect(
          SUPERVISOR_TRANSITION_TABLE[state][event],
          `missing ${state} × ${event}`
        ).toBeDefined();
      }
    }
  });

  it("only ever names states from the canonical set", () => {
    const states = new Set<string>(SUPERVISOR_STATES);
    for (const row of SUPERVISOR_TRANSITIONS) {
      expect(states.has(row.next), `${row.from} × ${row.event} → ${row.next}`).toBe(true);
    }
  });

  it("carries the canonical seven states, matching environmentStore's vocabulary", () => {
    expect([...SUPERVISOR_STATES]).toEqual([
      "idle",
      "connecting",
      "connected",
      "backoff",
      "offline",
      "blocked",
      "suspended",
    ]);
  });
});

describe("parking states never burn retry attempts", () => {
  it.each(["blocked", "suspended", "offline"] as const)(
    "no edge INTO %s schedules backoff",
    (parked) => {
      const entering = SUPERVISOR_TRANSITIONS.filter((row) => row.next === parked);
      expect(entering.length).toBeGreaterThan(0);
      for (const row of entering) {
        expect(
          row.effects,
          `${row.from} × ${row.event} → ${parked}`
        ).not.toContain("scheduleBackoff");
      }
    }
  );

  it.each(["blocked", "suspended", "offline"] as const)(
    "no edge OUT OF %s schedules backoff either",
    (parked) => {
      for (const event of SUPERVISOR_EVENTS) {
        expect(
          lookupTransition(parked, event).effects,
          `${parked} × ${event}`
        ).not.toContain("scheduleBackoff");
      }
    }
  );

  it("P-10: every blocked row is timer-free", () => {
    for (const event of SUPERVISOR_EVENTS) {
      const effects = lookupTransition("blocked", event).effects;
      expect(effects).not.toContain("scheduleBackoff");
      expect(effects).not.toContain("armStableTimer");
    }
  });
});

describe("blocked entry and wakeups", () => {
  const BLOCKED_CAUSES: SupervisorEvent[] = [
    "connect_failed_unauthorized",
    "connect_failed_version",
    "connect_failed_malformed_descriptor",
  ];

  it.each(BLOCKED_CAUSES)("connecting × %s → blocked", (event) => {
    const row = lookupTransition("connecting", event);
    expect(row.next).toBe("blocked");
    expect(row.effects).toEqual(["cancelAttempt", "releaseSocket", "clearTimers"]);
  });

  it("exposes exactly the four §6.5 wakeups out of blocked", () => {
    const wakeups = SUPERVISOR_EVENTS.filter(
      (event) => lookupTransition("blocked", event).next === "connecting"
    );
    expect([...wakeups].sort()).toEqual(
      ["credentials_changed", "online", "resume", "retry_now"].sort()
    );
  });

  it("suspend parks a blocked environment rather than clearing the block", () => {
    expect(lookupTransition("blocked", "suspend").next).toBe("suspended");
  });

  it("every blocked wakeup resets the ladder and begins one attempt", () => {
    for (const event of ["credentials_changed", "online", "resume", "retry_now"] as const) {
      expect(lookupTransition("blocked", event).effects).toEqual([
        "resetAttempts",
        "beginAttempt",
      ]);
    }
  });
});

describe("reaching connected", () => {
  it("only connect_succeeded promotes anything to connected", () => {
    const promoting = SUPERVISOR_TRANSITIONS.filter(
      (row) => row.next === "connected" && row.from !== "connected"
    );
    expect(promoting.map((row) => row.event)).toEqual(["connect_succeeded"]);
    expect(promoting[0]?.from).toBe("connecting");
  });

  it("P-25: no edge from suspended reaches connected — resume must run a full attempt", () => {
    for (const event of SUPERVISOR_EVENTS) {
      expect(lookupTransition("suspended", event).next).not.toBe("connected");
    }
    const resume = lookupTransition("suspended", "resume");
    expect(resume.next).toBe("connecting");
    expect(resume.effects).toEqual(["beginAttempt"]);
    // Parking burned nothing, so resuming must NOT reset the ladder — otherwise a
    // background/foreground cycle would defeat backoff entirely.
    expect(resume.effects).not.toContain("resetAttempts");
  });

  it("foreground while connected probes without leaving connected", () => {
    const row = lookupTransition("connected", "resume");
    expect(row.next).toBe("connected");
    expect(row.effects).toEqual(["probe"]);
  });

  it("socket loss while connected always releases and re-ladders", () => {
    const row = lookupTransition("connected", "socket_lost");
    expect(row.next).toBe("backoff");
    expect(row.effects).toEqual(["releaseSocket", "scheduleBackoff"]);
  });
});

describe("ladder reset discipline", () => {
  it("connecting successfully does NOT reset the ladder — only 30 s stable does", () => {
    expect(lookupTransition("connecting", "connect_succeeded").effects).toEqual([
      "armStableTimer",
    ]);
    expect(lookupTransition("connected", "stable_period_elapsed").effects).toEqual([
      "resetAttempts",
    ]);
  });

  it("going offline from backoff keeps the attempt count", () => {
    expect(lookupTransition("backoff", "offline").effects).not.toContain("resetAttempts");
  });
});

describe("retry ladder", () => {
  it("is 1s,2s,4s,8s,16s", () => {
    expect([...RETRY_LADDER_MS]).toEqual([1_000, 2_000, 4_000, 8_000, 16_000]);
  });

  it("clamps at 16 s forever", () => {
    expect(retryDelayMs(0)).toBe(1_000);
    expect(retryDelayMs(4)).toBe(16_000);
    expect(retryDelayMs(5)).toBe(16_000);
    expect(retryDelayMs(500)).toBe(16_000);
    expect(RETRY_LADDER_MAX_MS).toBe(RETRY_LADDER_MS[RETRY_LADDER_MS.length - 1]);
  });

  it("treats a negative attempt as the first rung rather than throwing", () => {
    expect(retryDelayMs(-3)).toBe(1_000);
  });

  it("pins the §6.5 budgets", () => {
    expect(CONNECT_BUDGET_MS).toBe(15_000);
    expect(STABLE_PERIOD_MS).toBe(30_000);
  });
});

describe("presentation projection", () => {
  it("maps each state to its §6.5 presentation", () => {
    const cases: [SupervisorState, boolean, string][] = [
      ["idle", false, "connecting"],
      ["connecting", false, "connecting"],
      ["connecting", true, "reconnecting"],
      ["connected", true, "connected"],
      ["backoff", false, "reconnecting"],
      ["backoff", true, "reconnecting"],
      ["offline", true, "offline"],
      ["blocked", true, "error"],
      ["suspended", true, "suspended"],
    ];
    for (const [state, hasEverConnected, expected] of cases) {
      expect(presentationFor(state, hasEverConnected)).toBe(expected);
    }
  });

  it("never presents a non-connected state as connected", () => {
    for (const state of SUPERVISOR_STATES) {
      if (state === "connected") continue;
      expect(presentationFor(state, true)).not.toBe("connected");
      expect(presentationFor(state, false)).not.toBe("connected");
    }
  });
});
