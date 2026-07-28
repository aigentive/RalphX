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
  bus: NetworkEventBus | null;
  detachRelay: (() => void) | null;
  relayKind: "full" | "health" | null;
}

const confirmedScopes = new Map<string, readonly string[]>();
let activeTeardown: (() => void) | null = null;

export function getConfirmedScopes(environmentId: string): readonly string[] | null {
  return confirmedScopes.get(environmentId) ?? null;
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
  let enabled = useUiStore.getState().featureFlags.remoteEnvironments;
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

  const buildActiveBus = (runtime: RuntimeEntry): void => {
    detachRelay(runtime);
    const environmentId = runtime.entry.id;
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
    runtime.bus = bus;
    runtime.detachRelay = attachRemoteStreamRelay({
      localBus,
      target: bus,
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
            confirmedScopes.get(environmentId) ??
            runtime.entry.remote?.scopes ??
            []
          );
        }
        const response = await networkFetch(environmentId, SESSION_PATH);
        if (!response.ok) {
          throw new Error(`Session introspection failed with HTTP ${response.status}.`);
        }
        return parseScopes(await response.json());
      },
      applyScopes: (scopes) => {
        confirmedScopes.set(environmentId, [...scopes]);
      },
      beginStream: async (outcome) => {
        if (useEnvironmentStore.getState().activeEnvironmentId !== environmentId) {
          return;
        }
        await runtime.bus?.beginStream(outcome);
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
      bus: null,
      detachRelay: null,
      relayKind: null,
    };
    return runtime;
  };

  const activate = (environmentId: string): void => {
    const previous = runtimes.get(activeEnvironmentId);
    const demoted = previous !== undefined && previous.entry.id !== environmentId;
    if (demoted) {
      previous.bus = null;
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
    buildActiveBus(runtime);
    // A fresh supervisor attempt pairs the fresh bus with the next hello H barrier.
    runtime.supervisor.stop();
    runtime.supervisor.start();
  };

  const quiesce = (): void => {
    for (const [environmentId, runtime] of runtimes) {
      runtime.supervisor.stop();
      detachRelay(runtime);
      runtime.bus = null;
      confirmedScopes.delete(environmentId);
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
        runtimes.delete(environmentId);
        confirmedScopes.delete(environmentId);
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
        buildActiveBus(runtime);
      } else {
        attachHealthRelay(runtime);
      }
      runtime.supervisor.start();
    }
  };

  registerRemoteEventBusFactory((environmentId, fallbackLocalBus) => {
    const runtime = runtimes.get(environmentId);
    return enabled &&
      environmentId === activeEnvironmentId &&
      runtime?.bus !== null &&
      runtime?.bus !== undefined
      ? runtime.bus
      : detachedBus(environmentId, fallbackLocalBus);
  });

  const unsubscribeUi = useUiStore.subscribe((state, previous) => {
    const next = state.featureFlags.remoteEnvironments;
    if (next === previous.featureFlags.remoteEnvironments) {
      return;
    }
    enabled = next;
    if (!enabled) {
      quiesce();
      return;
    }
    reconcile();
    activate(useEnvironmentStore.getState().activeEnvironmentId);
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
