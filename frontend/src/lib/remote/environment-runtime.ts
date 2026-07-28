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
  type RemoteClientFrame,
  type RemoteConnectOutcome,
  type RemoteServerFrame,
} from "./stream-frames";
import {
  ConnectionSupervisor,
  type EnvironmentDescriptorView,
} from "./supervisor";

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
  await primitiveInvoke("remote_stream_send", {
    input: { id: environmentId, frame },
  });
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
        await getQueryClient(environmentId).invalidateQueries();
      },
      sweep: () => {
        void getQueryClient(environmentId).invalidateQueries();
      },
      onRestartRequired: () => runtime.supervisor.streamLost(),
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
        const outcome = parseConnectOutcome(
          await primitiveInvoke("remote_connect", { input: { id: environmentId } })
        );
        runtime.socketLive = true;
        return outcome;
      },
      releaseStream: async () => {
        runtime.socketLive = false;
        await primitiveInvoke("remote_disconnect", { input: { id: environmentId } });
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
        useEnvironmentStore.getState().setConnectionState(environmentId, state);
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
    if (previous !== undefined && previous.entry.id !== environmentId) {
      previous.bus = null;
      attachHealthRelay(previous);
    }
    activeEnvironmentId = environmentId;
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
