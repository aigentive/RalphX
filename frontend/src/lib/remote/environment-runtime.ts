/**
 * App-lifetime composition root for registered remote environments.
 *
 * The environment-store isolation subscriber is installed at module import time; this
 * runtime subscribes later, at App initialization. Zustand therefore runs isolation
 * before the synchronous activation below builds the bus React will consume.
 */

import { invoke as primitiveInvoke } from "#tauri-core-primitive";

import {
  createEventBus,
  registerRemoteEventBusFactory,
  resetRemoteEventBusFactory,
  type EventBus,
} from "@/lib/event-bus";
import { getQueryClient, removeQueryClient } from "@/lib/queryClient";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
  type EnvironmentEntry,
} from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";

import { getClientOwnedFeatureFlag } from "./feature-flag-authority";

import { NetworkEventBus } from "./network-event-bus";
import { networkFetch } from "./network-fetch";
import { attachRemoteStreamRelay, type RemoteStreamTarget } from "./stream-relay";
import {
  isAuthorityResetReason,
  type RemoteClientFrame,
  type RemoteConnectOutcome,
  type RemoteServerFrame,
} from "./stream-frames";
import {
  ConnectionSupervisor,
  type EnvironmentDescriptorView,
} from "./supervisor";
import type { SupervisorState } from "./supervisor-transition-table";
import { toRemoteTransportError } from "./transport-errors";

const CLIENT_PROTOCOL_VERSION = 1;
const CLIENT_MIN_PROTOCOL = 1;
const DESCRIPTOR_PATH = "/.well-known/ralphx/environment";
const SESSION_PATH = "/remote/v1/session";
const HEALTH_PATH = "/health";

interface RuntimeEntry {
  entry: EnvironmentEntry;
  supervisor: ConnectionSupervisor;
  socketLive: boolean;
  /**
   * The environment's ONE bus instance, from the app-lifetime bus registry.
   *
   * `EventProvider` memoizes the bus on `environmentId` alone, so rebuilding it for a
   * same-id reactivation (a flag toggle, a re-activate) would leave the whole mounted
   * tree subscribed to an orphaned instance while the badge still read Connected.
   * Identity is therefore stable per environment; only the relay wiring is swapped.
   */
  bus: NetworkEventBus;
  detachRelay: (() => void) | null;
  relayKind: "full" | "health" | null;
}

let activeTeardown: (() => void) | null = null;

/**
 * The confirmed scope set lives on `environmentStore.effectiveScopes` and NOWHERE
 * else (PR 2.6-b).
 *
 * It used to be a module-local `Map` here, which meant the value gates need was
 * unreachable from React and would have had to be copied into the store — two
 * representations of one fact, drifting the moment either write path was missed.
 * Keeping one copy makes the store the single reader and this module the single
 * writer.
 */
export function getConfirmedScopes(environmentId: string): readonly string[] | null {
  return useEnvironmentStore.getState().effectiveScopes[environmentId] ?? null;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;
}

function parseDescriptor(value: unknown): EnvironmentDescriptorView {
  const record = asRecord(value);
  if (
    record === null ||
    typeof record.environmentId !== "string" ||
    typeof record.protocolVersion !== "number" ||
    typeof record.minClientProtocol !== "number"
  ) {
    throw new Error("Remote environment descriptor has an invalid shape.");
  }
  return {
    environmentId: record.environmentId,
    protocolVersion: record.protocolVersion,
    minClientProtocol: record.minClientProtocol,
  };
}

function parseScopes(value: unknown): readonly string[] {
  const record = asRecord(value);
  if (
    record === null ||
    !Array.isArray(record.scopes) ||
    !record.scopes.every((scope) => typeof scope === "string")
  ) {
    throw new Error("Remote session introspection has an invalid scope set.");
  }
  return record.scopes;
}

function parseConnectOutcome(value: unknown): RemoteConnectOutcome {
  const record = asRecord(value);
  if (
    record === null ||
    typeof record.environmentId !== "string" ||
    typeof record.hostEnvironmentId !== "string" ||
    typeof record.streamEpoch !== "string" ||
    typeof record.maxSeq !== "number" ||
    typeof record.heartbeatSecs !== "number" ||
    typeof record.protocolVersion !== "number"
  ) {
    throw new Error("Remote connect outcome has an invalid shape.");
  }
  return {
    environmentId: record.environmentId,
    hostEnvironmentId: record.hostEnvironmentId,
    streamEpoch: record.streamEpoch,
    maxSeq: record.maxSeq,
    heartbeatSecs: record.heartbeatSecs,
    protocolVersion: record.protocolVersion,
  };
}

async function sendFrame(
  environmentId: string,
  frame: RemoteClientFrame
): Promise<void> {
  try {
    await primitiveInvoke("remote_stream_send", {
      input: { id: environmentId, frame },
    });
  } catch (reason: unknown) {
    // The proxy rejects with the `"{CODE}: {message}"` rendering, which the
    // supervisor's typed classification cannot read as anything but `transient`.
    throw toRemoteTransportError(reason, environmentId, "remote_stream_send");
  }
}

function detachedBus(environmentId: string, localBus: EventBus): NetworkEventBus {
  return new NetworkEventBus({
    environmentId,
    localBus,
    sendFrame: async () => {},
    hydrate: async () => {},
    sweep: () => {},
    onRestartRequired: () => {},
  });
}

export function initializeEnvironmentRuntime(): () => void {
  if (activeTeardown !== null) {
    return activeTeardown;
  }

  const localBus = createEventBus(LOCAL_ENVIRONMENT_ID);
  const runtimes = new Map<string, RuntimeEntry>();
  // `remoteEnvironments` is CLIENT-owned: it decides whether THIS device runs the
  // remote runtime at all, so it must never be answered by a host. Read through the
  // authority helper rather than `uiStore` directly, so the constraint is stated
  // where it is relied on. See `feature-flag-authority.ts`.
  let enabled = getClientOwnedFeatureFlag("remoteEnvironments");
  let activeEnvironmentId = useEnvironmentStore.getState().activeEnvironmentId;

  /**
   * The single writer of `connectionStates`, and the one place the P-25 "never a probe
   * alone" rule is enforced for BACKGROUND environments.
   *
   * A non-active environment completes descriptor + socket + hello + probe, but its
   * `beginStream` is a no-op: no `subscribe` frame is ever sent and nothing is
   * projected (full background projection is a v1 non-goal). Painting that green would
   * assert a stream liveness that does not exist, so it presents as `health_only`.
   */
  const publishConnectionState = (
    environmentId: string,
    state: SupervisorState
  ): void => {
    useEnvironmentStore
      .getState()
      .setConnectionState(
        environmentId,
        state === "connected" && environmentId !== activeEnvironmentId
          ? "health_only"
          : state
      );
  };

  const detachRelay = (runtime: RuntimeEntry): void => {
    runtime.detachRelay?.();
    runtime.detachRelay = null;
    runtime.relayKind = null;
  };

  const streamClosed = (runtime: RuntimeEntry): void => {
    runtime.socketLive = false;
    runtime.supervisor.streamLost();
  };

  const attachHealthRelay = (runtime: RuntimeEntry): void => {
    if (runtime.relayKind === "health") {
      return;
    }
    detachRelay(runtime);
    const target: RemoteStreamTarget = {
      environmentId: () => runtime.entry.id,
      handleFrame: (frame: RemoteServerFrame) => {
        if (frame.type === "heartbeat") {
          void sendFrame(runtime.entry.id, {
            type: "heartbeatAck",
            t: frame.t,
          });
        }
        // Full background projection is a v1 non-goal: every non-heartbeat frame drops.
      },
      handleStreamClosed: () => {
        runtime.socketLive = false;
      },
    };
    runtime.detachRelay = attachRemoteStreamRelay({
      localBus,
      target,
      onFrameActivity: () => runtime.supervisor.noteFrameActivity(),
      onStreamClosed: () => streamClosed(runtime),
    });
    runtime.relayKind = "health";
  };

  /**
   * The app-lifetime bus registry: exactly ONE `NetworkEventBus` per environment id,
   * outliving deactivation, flag toggles, and runtime reconciliation. Only a removed
   * registry row forgets its bus.
   */
  const buses = new Map<string, NetworkEventBus>();

  const projectionBus = (environmentId: string): NetworkEventBus => {
    const existing = buses.get(environmentId);
    if (existing !== undefined) {
      return existing;
    }
    const bus = new NetworkEventBus({
      environmentId,
      localBus,
      sendFrame: (frame) => sendFrame(environmentId, frame),
      hydrate: async () => {
        // The §3.4 hydration barrier must FAIL CLOSED: TanStack swallows every refetch
        // rejection unless `throwOnError`, so a 500ing host would resolve the barrier,
        // validate the cursor, and let the badge read Connected over an empty board.
        // `refetchType: "all"` because a snapshot is not "taken" if the inactive
        // queries the next render will read were left stale.
        await getQueryClient(environmentId).invalidateQueries(
          { refetchType: "all" },
          { throwOnError: true }
        );
      },
      sweep: () => {
        void getQueryClient(environmentId).invalidateQueries();
      },
      onRestartRequired: (cause) => {
        // Resolved at call time, never captured: the bus outlives any single runtime.
        const runtime = runtimes.get(environmentId);
        if (runtime === undefined) {
          return;
        }
        // `revoked` / `host_disabled` are the host WITHDRAWING the session. Routing
        // them to `streamLost` would redial the 16 s ladder forever against a host
        // that already refused this device, instead of showing the re-pair state.
        if (cause.kind === "reset" && isAuthorityResetReason(cause.reason)) {
          runtime.supervisor.authorityWithdrawn(
            `The host ended this device's session (${cause.reason}). Re-pair this environment to reconnect.`
          );
          return;
        }
        runtime.supervisor.streamLost();
      },
    });
    buses.set(environmentId, bus);
    return bus;
  };

  /** Points the environment's stable bus at the live relay and starts projecting. */
  const attachProjectionRelay = (runtime: RuntimeEntry): void => {
    detachRelay(runtime);
    // Whatever the host did while this bus was not projecting is unobserved, so the
    // next attempt must cold-hydrate rather than resume a cursor it stopped honouring.
    runtime.bus.abandonStream();
    runtime.detachRelay = attachRemoteStreamRelay({
      localBus,
      target: runtime.bus,
      onFrameActivity: () => runtime.supervisor.noteFrameActivity(),
      onStreamClosed: () => streamClosed(runtime),
      onUndecodableFrame: () => runtime.supervisor.streamLost(),
    });
    runtime.relayKind = "full";
  };

  const createRuntime = (entry: EnvironmentEntry): RuntimeEntry => {
    let runtime: RuntimeEntry;
    const environmentId = entry.id;
    const supervisor = new ConnectionSupervisor({
      environmentId,
      expectedHostEnvironmentId: entry.remote?.environmentId ?? environmentId,
      clientProtocolVersion: CLIENT_PROTOCOL_VERSION,
      clientMinProtocol: CLIENT_MIN_PROTOCOL,
      fetchDescriptor: async () => {
        const response = await networkFetch(environmentId, DESCRIPTOR_PATH);
        if (!response.ok) {
          throw new Error(`Descriptor request failed with HTTP ${response.status}.`);
        }
        return parseDescriptor(await response.json());
      },
      openStream: async () => {
        let raw: unknown;
        try {
          raw = await primitiveInvoke("remote_connect", {
            input: { id: environmentId },
          });
        } catch (reason: unknown) {
          // A revoked credential must reach `classifyFailure` as REMOTE_UNAUTHORIZED,
          // not as an untyped string the ladder retries forever.
          throw toRemoteTransportError(reason, environmentId, "remote_connect");
        }
        const outcome = parseConnectOutcome(raw);
        runtime.socketLive = true;
        return outcome;
      },
      releaseStream: async () => {
        runtime.socketLive = false;
        try {
          await primitiveInvoke("remote_disconnect", { input: { id: environmentId } });
        } catch (reason: unknown) {
          throw toRemoteTransportError(reason, environmentId, "remote_disconnect");
        }
      },
      probe: async () => {
        const response = await networkFetch(environmentId, HEALTH_PATH);
        if (!response.ok) {
          throw new Error(`Health probe failed with HTTP ${response.status}.`);
        }
      },
      refreshScopes: async () => {
        if (useEnvironmentStore.getState().activeEnvironmentId !== environmentId) {
          // PR 3.3 owns background introspection. Until then, never make an
          // active-env-bound session request: retain confirmed or pairing-time scopes.
          return (
            getConfirmedScopes(environmentId) ?? runtime.entry.remote?.scopes ?? []
          );
        }
        const response = await networkFetch(environmentId, SESSION_PATH);
        if (!response.ok) {
          throw new Error(`Session introspection failed with HTTP ${response.status}.`);
        }
        return parseScopes(await response.json());
      },
      applyScopes: (scopes) => {
        useEnvironmentStore.getState().setEffectiveScopes(environmentId, scopes);
      },
      beginStream: async (outcome) => {
        if (useEnvironmentStore.getState().activeEnvironmentId !== environmentId) {
          return;
        }
        await runtime.bus.beginStream(outcome);
      },
      hasLiveSocket: () => runtime.socketLive,
      onStateChange: (state) => {
        publishConnectionState(environmentId, state);
      },
    });
    runtime = {
      entry,
      supervisor,
      socketLive: false,
      bus: projectionBus(environmentId),
      detachRelay: null,
      relayKind: null,
    };
    return runtime;
  };

  const activate = (environmentId: string): void => {
    const previous = runtimes.get(activeEnvironmentId);
    const demoted = previous !== undefined && previous.entry.id !== environmentId;
    if (demoted) {
      // The instance survives — React holds it — but it stops projecting.
      previous.bus.abandonStream();
      attachHealthRelay(previous);
    }
    activeEnvironmentId = environmentId;
    // Persistence never substitutes for the cold hydrate on reactivation. The target
    // environment's QueryClient is RETAINED across switches, so without this every
    // remounted query is inside its 5-minute staleTime and the board renders minutes
    // -old data as current — for `local` (which has no supervisor to re-hydrate it)
    // and for any remote environment whose supervisor is parked in backoff/blocked.
    void getQueryClient(environmentId).invalidateQueries();
    if (demoted) {
      // The demoted environment stops projecting the instant it loses the bus, so its
      // badge must stop claiming a live stream even though its FSM state is unchanged.
      publishConnectionState(previous.entry.id, previous.supervisor.currentState());
    }
    const runtime = runtimes.get(environmentId);
    if (runtime === undefined) {
      return;
    }
    attachProjectionRelay(runtime);
    // A fresh supervisor attempt pairs the reset bus with the next hello H barrier.
    runtime.supervisor.stop();
    runtime.supervisor.start();
  };

  const quiesce = (): void => {
    for (const [environmentId, runtime] of runtimes) {
      runtime.supervisor.stop();
      detachRelay(runtime);
      runtime.bus.abandonStream();
      useEnvironmentStore.getState().clearEffectiveScopes(environmentId);
    }
    runtimes.clear();
  };

  const reconcile = (): void => {
    const state = useEnvironmentStore.getState();
    const remoteEntries = state.environments.filter(
      (entry) => entry.kind === "remote"
    );
    const wanted = new Set(remoteEntries.map((entry) => entry.id));

    for (const [environmentId, runtime] of runtimes) {
      if (!wanted.has(environmentId)) {
        runtime.supervisor.stop();
        detachRelay(runtime);
        runtime.bus.abandonStream();
        runtimes.delete(environmentId);
        // The registry row is gone, so this environment's bus identity may go too.
        buses.delete(environmentId);
        useEnvironmentStore.getState().clearEffectiveScopes(environmentId);
        removeQueryClient(environmentId);
      }
    }
    for (const entry of remoteEntries) {
      const existing = runtimes.get(entry.id);
      if (existing !== undefined) {
        existing.entry = entry;
        continue;
      }
      const runtime = createRuntime(entry);
      runtimes.set(entry.id, runtime);
      if (entry.id === state.activeEnvironmentId) {
        attachProjectionRelay(runtime);
      } else {
        attachHealthRelay(runtime);
      }
      runtime.supervisor.start();
    }
  };

  registerRemoteEventBusFactory((environmentId, fallbackLocalBus) => {
    // `relayKind`, not bus existence: the bus is permanent, so what decides whether a
    // consumer gets the real projector is whether it is currently wired to the relay.
    const runtime = runtimes.get(environmentId);
    return enabled &&
      environmentId === activeEnvironmentId &&
      runtime !== undefined &&
      runtime.relayKind === "full"
      ? runtime.bus
      : detachedBus(environmentId, fallbackLocalBus);
  });

  /**
   * Pulls the durable registry from Rust into the store.
   *
   * This is the ONLY unprompted reader of the registry. `loadEnvironments` first, then
   * `hydrateActiveEnvironment`: the active id Rust reports is meaningless until the
   * list that can contain it is present, and hydrating first would see an unknown id
   * and clamp the mirror to local — telling Rust to abandon an environment it is still
   * authorizing.
   *
   * Failures are swallowed deliberately. A registry that cannot be read shows no remote
   * environments at all, which is the fail-closed presentation; there is no retry here
   * (A-5: the supervisor is the sole retry owner) and the user re-opens Connections.
   */
  const loadRegistry = (): void => {
    if (!enabled) {
      return;
    }
    void (async () => {
      const store = useEnvironmentStore.getState();
      await store.loadEnvironments();
      await store.hydrateActiveEnvironment();
    })().catch((error: unknown) => {
      console.warn("[remote] registry load failed; no remote environments shown", error);
    });
  };

  // Subscribes to `uiStore` deliberately: this is the CLIENT-owned copy of the flag
  // (see `feature-flag-authority.ts`). The env-scoped `useFeatureFlags` query must
  // never drive this, or a host would control whether this device does remote.
  const unsubscribeUi = useUiStore.subscribe((state, previous) => {
    const next = state.featureFlags.remoteEnvironments === true;
    if (next === (previous.featureFlags.remoteEnvironments === true)) {
      return;
    }
    enabled = next;
    if (!enabled) {
      quiesce();
      return;
    }
    reconcile();
    activate(useEnvironmentStore.getState().activeEnvironmentId);
    loadRegistry();
  });

  const unsubscribeEnvironment = useEnvironmentStore.subscribe((state, previous) => {
    if (!enabled) {
      return;
    }
    if (state.environments !== previous.environments) {
      reconcile();
    }
    if (state.activeEnvironmentId !== previous.activeEnvironmentId) {
      activate(state.activeEnvironmentId);
    }
  });

  const visibilityChanged = (): void => {
    for (const runtime of runtimes.values()) {
      runtime.supervisor.visibilityChanged(document.hidden);
    }
  };
  const online = (): void => {
    for (const runtime of runtimes.values()) {
      runtime.supervisor.networkChanged(true);
    }
  };
  const offline = (): void => {
    for (const runtime of runtimes.values()) {
      runtime.supervisor.networkChanged(false);
    }
  };
  document.addEventListener("visibilitychange", visibilityChanged);
  window.addEventListener("online", online);
  window.addEventListener("offline", offline);

  if (enabled) {
    reconcile();
    activate(activeEnvironmentId);
    loadRegistry();
  }

  const teardown = (): void => {
    if (activeTeardown !== teardown) {
      return;
    }
    unsubscribeUi();
    unsubscribeEnvironment();
    document.removeEventListener("visibilitychange", visibilityChanged);
    window.removeEventListener("online", online);
    window.removeEventListener("offline", offline);
    quiesce();
    resetRemoteEventBusFactory();
    activeTeardown = null;
  };
  activeTeardown = teardown;
  return teardown;
}
