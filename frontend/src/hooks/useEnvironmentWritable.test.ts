/**
 * Read-only mode (2.7-a) and its fold into the 2.6 gate seam (Decision 4).
 *
 * Fake timers: the point of read-only is that a refused mutation fails FAST and nothing
 * is scheduled to try it again later (A-5, and there is no offline outbox in v1).
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentGate } from "@/hooks/useAgentGate";
import { useEnvironmentWritable } from "@/hooks/useEnvironmentWritable";
import { UI_AGENT_SCOPE } from "@/lib/remote/agent-gate";
import type { SupervisorPresentation } from "@/lib/remote/supervisor-transition-table";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

const REMOTE_ID = "env-studio";
const REMOTE_NAME = "Studio Mac";

function seed({
  presentation,
  active = REMOTE_ID,
  scopes = [UI_AGENT_SCOPE],
}: {
  presentation?: SupervisorPresentation;
  active?: string;
  scopes?: readonly string[];
}): void {
  act(() => {
    useEnvironmentStore.setState({
      activeEnvironmentId: active,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
        { id: REMOTE_ID, name: REMOTE_NAME, kind: "remote" },
      ],
      effectiveScopes: { [REMOTE_ID]: scopes },
      connectionPresentations:
        presentation === undefined
          ? {}
          : {
              [REMOTE_ID]: {
                presentation,
                blockedFailure: null,
                blockedMessage: null,
              },
            },
    });
  });
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  expect(
    vi.getTimerCount(),
    "read-only enforcement scheduled a timer; nothing may queue a refused mutation"
  ).toBe(0);
  vi.useRealTimers();
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    connectionPresentations: {},
    effectiveScopes: {},
  });
});

describe("useEnvironmentWritable", () => {
  it("keeps the local environment writable — it has no transport to lose", () => {
    seed({ active: LOCAL_ENVIRONMENT_ID });
    const { result } = renderHook(() => useEnvironmentWritable());

    expect(result.current).toEqual({ writable: true, reason: null });
  });

  it("is writable only while the remote supervisor says connected", () => {
    seed({ presentation: "connected" });
    const { result } = renderHook(() => useEnvironmentWritable());

    expect(result.current.writable).toBe(true);
  });

  it.each<[SupervisorPresentation, RegExp]>([
    ["reconnecting", /reconnecting/i],
    ["offline", /offline/i],
    ["connecting", /still connecting/i],
    // Still read-only — calm chrome is not a confirmed connection — but the copy must
    // match the syncing chip, never claim a dropped connection.
    ["syncing", /syncing/i],
    ["suspended", /paused in the background/i],
    ["error", /needs attention/i],
  ])("locks writes while %s and says why", (presentation, expected) => {
    seed({ presentation });
    const { result } = renderHook(() => useEnvironmentWritable());

    expect(result.current.writable).toBe(false);
    expect(result.current.reason).toMatch(expected);
    expect(result.current.reason).toContain(REMOTE_NAME);
  });

  it("fails CLOSED when no supervisor has reported yet", () => {
    seed({});
    const { result } = renderHook(() => useEnvironmentWritable());

    expect(result.current.writable).toBe(false);
    expect(result.current.reason).toContain(REMOTE_NAME);
  });

  it("becomes writable again the moment the connection returns", () => {
    seed({ presentation: "reconnecting" });
    const { result } = renderHook(() => useEnvironmentWritable());
    expect(result.current.writable).toBe(false);

    seed({ presentation: "connected" });

    expect(result.current.writable).toBe(true);
  });
});

describe("useAgentGate read-only fold (Decision 4)", () => {
  it("gates an authorized affordance while the connection is degraded", () => {
    seed({ presentation: "connected" });
    const { result } = renderHook(() => useAgentGate("taskMove"));
    // Baseline: fully authorized on a healthy connection.
    expect(result.current.gated).toBe(false);

    seed({ presentation: "reconnecting" });

    expect(result.current.status).toBe("read_only");
    expect(result.current.gated).toBe(true);
    expect(result.current.reason).toMatch(/reconnecting/i);
  });

  it("folds a reachable resume twin into read-only while reconnecting", () => {
    seed({ presentation: "reconnecting" });
    const { result } = renderHook(() => useAgentGate("taskResume"));

    expect(result.current.status).toBe("read_only");
    expect(result.current.gated).toBe(true);
    expect(result.current.reason).toMatch(/reconnecting/i);
  });

  it("leaves the local environment fully enabled", () => {
    seed({ active: LOCAL_ENVIRONMENT_ID, presentation: "offline" });
    const { result } = renderHook(() => useAgentGate("taskMove"));

    expect(result.current).toEqual({
      status: "enabled",
      gated: false,
      reason: null,
    });
  });
});
