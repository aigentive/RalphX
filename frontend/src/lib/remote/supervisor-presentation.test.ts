/**
 * Pure boundary tests for the `syncing` projection rule. The FSM-integration half
 * (emission points, escalation timer, one-way-ness) lives in `supervisor.test.ts`.
 */

import { describe, expect, it } from "vitest";

import {
  MAX_SYNCING_BARRIER_FAILURES,
  NO_SYNCING_HINT,
  SYNCING_GRACE_MS,
  presentationFor,
  type SyncingHint,
} from "./supervisor-presentation";
import { SUPERVISOR_STATES } from "./supervisor-transition-table";

/** A hint that satisfies every syncing condition; tests negate one at a time. */
const SYNCABLE: SyncingHint = {
  streamOpen: true,
  attempts: 3,
  barrierFailures: 0,
  episodeElapsedMs: 1_000,
  deadHostSuspected: false,
};

describe("syncing projection rule", () => {
  it("produces syncing across the whole disconnect band when all conditions hold", () => {
    for (const state of ["idle", "connecting", "backoff"] as const) {
      expect(presentationFor(state, true, SYNCABLE)).toBe("syncing");
    }
  });

  it("never presents syncing before the environment has ever connected", () => {
    for (const state of SUPERVISOR_STATES) {
      expect(presentationFor(state, false, SYNCABLE)).not.toBe("syncing");
    }
  });

  it("keeps the pinned legacy mapping under the inert hint", () => {
    // The default third argument is load-bearing: the checked-in projection table in
    // supervisor-transition-table.test.ts calls the two-argument form and must keep
    // passing verbatim.
    expect(presentationFor("connecting", false)).toBe("connecting");
    expect(presentationFor("connecting", true)).toBe("reconnecting");
    expect(presentationFor("backoff", true)).toBe("reconnecting");
    expect(presentationFor("backoff", true, NO_SYNCING_HINT)).toBe("reconnecting");
  });

  it("escalates on the K boundary: 1 barrier failure syncs, 2 do not", () => {
    expect(
      presentationFor("connecting", true, { ...SYNCABLE, barrierFailures: 1 })
    ).toBe("syncing");
    expect(
      presentationFor("connecting", true, {
        ...SYNCABLE,
        barrierFailures: MAX_SYNCING_BARRIER_FAILURES,
      })
    ).toBe("reconnecting");
  });

  it("escalates on the T boundary: exactly at the grace syncs, past it does not", () => {
    expect(
      presentationFor("backoff", true, {
        ...SYNCABLE,
        episodeElapsedMs: SYNCING_GRACE_MS,
      })
    ).toBe("syncing");
    expect(
      presentationFor("backoff", true, {
        ...SYNCABLE,
        episodeElapsedMs: SYNCING_GRACE_MS + 1,
      })
    ).toBe("reconnecting");
  });

  it("never syncs without a tracked episode", () => {
    expect(
      presentationFor("connecting", true, { ...SYNCABLE, episodeElapsedMs: null })
    ).toBe("reconnecting");
  });

  it("never syncs when the host is suspected dead", () => {
    expect(
      presentationFor("connecting", true, { ...SYNCABLE, deadHostSuspected: true })
    ).toBe("reconnecting");
  });

  it("requires an open stream once the ladder has burned more than one attempt", () => {
    const closed = { ...SYNCABLE, streamOpen: false };
    expect(presentationFor("connecting", true, { ...closed, attempts: 0 })).toBe(
      "syncing"
    );
    expect(presentationFor("connecting", true, { ...closed, attempts: 1 })).toBe(
      "syncing"
    );
    expect(presentationFor("connecting", true, { ...closed, attempts: 2 })).toBe(
      "reconnecting"
    );
  });

  it("never rewrites the non-band states", () => {
    expect(presentationFor("connected", true, SYNCABLE)).toBe("connected");
    expect(presentationFor("offline", true, SYNCABLE)).toBe("offline");
    expect(presentationFor("blocked", true, SYNCABLE)).toBe("error");
    expect(presentationFor("suspended", true, SYNCABLE)).toBe("suspended");
  });
});
