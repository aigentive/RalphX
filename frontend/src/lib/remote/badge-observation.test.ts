/**
 * PR 3.3-a — per-environment notification badges from BACKGROUND observation.
 *
 * Discharges:
 * - A-12: a background environment's warm cursor never advances, and nothing is
 *   persisted. Badge counting is an observation, not a projection.
 * - P-26: a background environment issues no invoke beyond the health ops the Rust
 *   proxy authorizes (descriptor + health). Counting frames adds no query.
 * - P-14: the local environment is untouched by remote badge traffic.
 * - WRITER: exactly one writer of `notificationBadges` — the composition root.
 *
 * The absence assertions here are the point of the file. A badge feature that
 * "works" while quietly advancing `lastSeq` would pass a count-only test and
 * silently convert every reactivation from a cold hydrate into a warm resume
 * across a gap it never projected.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteEnvironmentSummary } from "@/api/remote-environments";
import { getQueryClient, resetQueryClient } from "@/lib/queryClient";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";

import {
  BADGE_COUNTED_EVENT_NAME,
  countsTowardBadge,
} from "./badge-observation";
import {
  REMOTE_STREAM_FRAME_EVENT,
  type RemoteServerFrame,
  type RemoteStreamFrameEnvelope,
} from "./stream-frames";

/**
 * One shared local bus for the whole module, so a test can emit the relay frame the
 * Rust proxy would republish. Production hands the composition root a private
 * `MockEventBus` per call; without this the relay subscribes to a bus no test can
 * reach.
 */
const { sharedLocalBus } = vi.hoisted(() => ({
  sharedLocalBus: { current: null as { emit: (e: string, p: unknown) => void } | null },
}));

vi.mock("@/lib/event-bus", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/event-bus")>();
  const shared = new actual.MockEventBus();
  sharedLocalBus.current = shared;
  return {
    ...actual,
    createEventBus: (id: string = LOCAL_ENVIRONMENT_ID) =>
      id === LOCAL_ENVIRONMENT_ID ? shared : actual.createEventBus(id),
  };
});

vi.mock("./network-fetch", () => ({ networkFetch: vi.fn() }));
vi.mock("#tauri-core-primitive", () => ({ invoke: vi.fn(async () => undefined) }));

function summary(id: string): RemoteEnvironmentSummary {
  return {
    id,
    environmentId: `host-${id}`,
    name: id,
    baseUrl: `https://${id}.example.test`,
    candidateUrls: [],
    scopes: ["ui:read"],
    protocolVersion: 1,
    status: "active",
    createdAt: "2026-07-28T00:00:00Z",
    lastConnectedAt: null,
  };
}

const OUTCOME_C = {
  environmentId: "env-c",
  hostEnvironmentId: "host-env-c",
  streamEpoch: "epoch-1",
  maxSeq: 100,
  heartbeatSecs: 20,
  protocolVersion: 1,
};

function notificationFrame(seq: number | null): RemoteServerFrame {
  return {
    type: "event",
    seq,
    name: BADGE_COUNTED_EVENT_NAME,
    payload: { id: `n-${seq ?? "live"}` },
  };
}

function deliver(environmentId: string, frame: RemoteServerFrame): void {
  const envelope: RemoteStreamFrameEnvelope = { environmentId, frame };
  sharedLocalBus.current!.emit(REMOTE_STREAM_FRAME_EVENT, envelope);
}

function setFlag(enabled: boolean): void {
  const flags = useUiStore.getState().featureFlags;
  useUiStore.setState({ featureFlags: { ...flags, remoteEnvironments: enabled } });
}

function resetStores(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
    notificationBadges: {},
  });
  setFlag(false);
}

let teardown: (() => void) | null = null;

beforeEach(() => {
  resetStores();
  resetQueryClient();
});

afterEach(() => {
  teardown?.();
  teardown = null;
  resetStores();
  resetQueryClient();
});

/** Boots the runtime with `env-b` active and `env-c` in the background. */
async function boot(): Promise<void> {
  const { initializeEnvironmentRuntime } = await import("./environment-runtime");
  useEnvironmentStore
    .getState()
    .setEnvironments([summary("env-b"), summary("env-c")]);
  setFlag(true);
  teardown = initializeEnvironmentRuntime();
  useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
}

describe("countsTowardBadge", () => {
  it("counts only durable notification:created frames", () => {
    expect(countsTowardBadge(notificationFrame(7))).toBe(true);
  });

  it("never counts a transient frame — a live-only frame has no durable identity", () => {
    // A transient frame carries no seq, so it cannot be reconciled against hydrated
    // notification state on reactivation. Counting it would double-count.
    expect(countsTowardBadge(notificationFrame(null))).toBe(false);
  });

  it("never counts an unrelated durable event", () => {
    expect(
      countsTowardBadge({ type: "event", seq: 9, name: "task:updated", payload: {} })
    ).toBe(false);
  });

  it("never counts a non-event frame", () => {
    expect(countsTowardBadge({ type: "heartbeat", t: 1 })).toBe(false);
    expect(countsTowardBadge({ type: "reset", reason: "cursor_pruned" })).toBe(false);
  });
});

describe("A-12: background badge observation never projects", () => {
  it("counts a background environment's notifications", async () => {
    await boot();

    deliver("env-c", notificationFrame(11));
    deliver("env-c", notificationFrame(12));

    expect(useEnvironmentStore.getState().notificationBadges["env-c"]).toBe(2);
  });

  /**
   * The load-bearing assertion: it must FAIL if the relay ever forwards an observed
   * frame to the environment's bus.
   *
   * Two traps this test exists to avoid, both of which produce a green-but-vacuous
   * check:
   *
   * 1. Asserting "`lastSeq` did not move" ALONE proves nothing. A background bus has
   *    `cursorValid === false`, so `applyEvent` refuses to advance whatever it is
   *    handed — a relay that wrongly projected every frame would still pass.
   * 2. `createEventBus(id)` for a BACKGROUND environment returns a fresh detached bus,
   *    not the runtime's instance. Spying on that would watch an object the relay
   *    never touches.
   *
   * So the real instance is captured while the environment is still projecting, held
   * across the demotion, and only then spied on.
   */
  it("discards a demoted environment's cursor and never projects an observed frame", async () => {
    await boot();
    // Make env-c the projecting environment first, so it has a real bus and a real
    // cursor to lose.
    useEnvironmentStore.setState({ activeEnvironmentId: "env-c" });
    const { createEventBus } = await import("@/lib/event-bus");
    const bus = createEventBus("env-c") as unknown as {
      beginStream: (outcome: typeof OUTCOME_C) => Promise<void>;
      handleFrame: (frame: RemoteServerFrame) => void;
      cursor: () => { lastSeq: number; lastAckedSeq: number; valid: boolean };
    };
    await bus.beginStream(OUTCOME_C);
    expect(bus.cursor().lastSeq).toBe(100);
    expect(bus.cursor().valid).toBe(true);

    // Demote it: env-b takes the foreground. The cursor must be discarded here, which
    // is what forces the next activation to be a full `H` cold hydrate.
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    expect(bus.cursor().lastSeq).toBe(0);
    expect(bus.cursor().valid).toBe(false);

    const projected = vi.spyOn(bus, "handleFrame");
    deliver("env-c", notificationFrame(101));
    deliver("env-c", notificationFrame(102));

    // Counted, not projected.
    expect(projected).not.toHaveBeenCalled();
    expect(useEnvironmentStore.getState().notificationBadges["env-c"]).toBe(2);
    expect(bus.cursor().lastSeq).toBe(0);
    expect(bus.cursor().valid).toBe(false);
  });

  it("never mutates the background environment's retained cache", async () => {
    await boot();
    const backgroundInvalidate = vi.spyOn(
      getQueryClient("env-c"),
      "invalidateQueries"
    );
    const activeInvalidate = vi.spyOn(getQueryClient("env-b"), "invalidateQueries");

    deliver("env-c", notificationFrame(11));

    // Observation is not projection: no cache write on either environment.
    expect(backgroundInvalidate).not.toHaveBeenCalled();
    expect(activeInvalidate).not.toHaveBeenCalled();
  });

  it("never issues an invoke for a background environment (P-26)", async () => {
    await boot();
    const { invoke } = await import("#tauri-core-primitive");
    const { networkFetch } = await import("./network-fetch");
    vi.mocked(invoke).mockClear();
    vi.mocked(networkFetch).mockClear();

    deliver("env-c", notificationFrame(11));
    deliver("env-c", notificationFrame(12));

    // Badges come from frames already on the background socket. Deriving them from a
    // query would need a non-health proxy op, which `authorize_proxy_target` denies.
    expect(invoke).not.toHaveBeenCalled();
    expect(networkFetch).not.toHaveBeenCalled();
  });

  it("leaves the local environment alone (P-14)", async () => {
    await boot();

    deliver("env-c", notificationFrame(11));

    expect(
      useEnvironmentStore.getState().notificationBadges[LOCAL_ENVIRONMENT_ID]
    ).toBeUndefined();
  });

  it("does not count frames addressed to another environment", async () => {
    await boot();

    deliver("env-c", notificationFrame(11));

    expect(useEnvironmentStore.getState().notificationBadges["env-b"]).toBeUndefined();
  });
});

describe("reactivation reconciles the badge", () => {
  it("clears the accumulated count when the environment becomes active", async () => {
    await boot();
    deliver("env-c", notificationFrame(11));
    deliver("env-c", notificationFrame(12));
    expect(useEnvironmentStore.getState().notificationBadges["env-c"]).toBe(2);

    useEnvironmentStore.setState({ activeEnvironmentId: "env-c" });

    // The cold hydrate re-reads real notification state, so the observed tally has
    // done its job. Keeping it would double-count against the hydrated badge.
    expect(useEnvironmentStore.getState().notificationBadges["env-c"]).toBeUndefined();
  });

  it("stops counting once the environment is active and projecting", async () => {
    await boot();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-c" });

    deliver("env-c", notificationFrame(13));

    expect(useEnvironmentStore.getState().notificationBadges["env-c"]).toBeUndefined();
  });

  it("forgets a removed environment's badge", async () => {
    await boot();
    deliver("env-c", notificationFrame(11));

    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);

    // A re-added row can reuse an id; inheriting a stale tally would claim
    // notifications that belong to a connection that no longer exists.
    expect(useEnvironmentStore.getState().notificationBadges["env-c"]).toBeUndefined();
  });
});
