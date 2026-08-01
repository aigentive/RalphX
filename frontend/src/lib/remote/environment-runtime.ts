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
import {
  clearConnectionJournal,
  recordConnectionEvent,
} from "@/stores/remoteConnectionJournalStore";
import { useUiStore } from "@/stores/uiStore";

import { countsTowardBadge } from "./badge-observation";
import { clearEnvScopedStorage } from "./env-scoped-storage";
import { getClientOwnedFeatureFlag } from "./feature-flag-authority";

import { NetworkEventBus } from "./network-event-bus";
import { networkFetch } from "./network-fetch";
import { requestPendingGateReconcile } from "./pending-gate-reconcile";
import { attachRemoteStreamRelay, type RemoteStreamTarget } from "./stream-relay";
import {
  isAuthorityResetReason,
  type RemoteClientFrame,
  type RemoteConnectOutcome,
} from "./stream-frames";
import {
  ConnectionSupervisor,
  type EnvironmentDescriptorView,
} from "./supervisor";
import type { SupervisorState } from "./supervisor-transition-table";
import { RemoteTransportError, toRemoteTransportError } from "./transport-errors";

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
 * The runtime's user-facing actions, published by the live composition root.
 *
 * A module-level handle rather than a React context: the runtime is app-lifetime and
 * mounted imperatively (`initializeEnvironmentRuntime`), so a context provider would
 * only re-wrap what already exists. Components stay READERS of the store and callers
 * of these actions; none of them holds a supervisor.
 */
interface RuntimeActions {
  readonly retryNow: (environmentId: string) => void;
}

let runtimeActions: RuntimeActions | null = null;

/**
 * The ONE user-driven wakeup (A-5: the supervisor is the sole retry owner, and this is
 * not a retry — it is the user telling a parked FSM to try again). A no-op when no
 * runtime is live or the id has no supervisor, so a stale banner click cannot resurrect
 * a torn-down environment.
 */
export function retryActiveEnvironmentNow(environmentId: string): void {
  runtimeActions?.retryNow(environmentId);
}

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

/** One-line rendering of an unknown throw for the journal's `detail` column. */
function describeError(reason: unknown): string {
  if (reason instanceof RemoteTransportError) {
    return `${reason.code}: ${reason.message}`;
  }
  if (reason instanceof Error) {
    return reason.message;
  }
  return String(reason);
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
      handleFrame: (frame) => {
        // OBSERVE, NEVER PROJECT (A-12, §6.4). This relay counts; it does not apply.
        //
        // The frame is not handed to `runtime.bus`, so nothing here can advance
        // `lastSeq`, send a `cursorAck`, or reach the environment's retained
        // QueryClient — reactivation stays a full `H = hello.maxSeq` cold hydrate. The
        // count is a UI affordance derived from traffic already on a socket this
        // environment holds, which is why it needs no invoke and cannot erode P-26.
        //
        // Full background projection remains a v1 non-goal, and the heartbeat ack is
        // still NOT this relay's job: the Rust relay already acks each heartbeat on
        // the socket it owns (`remote_event_relay.rs`), precisely so a busy webview
        // can never starve the host's 2-unacked budget.
        if (countsTowardBadge(frame)) {
          useEnvironmentStore
            .getState()
            .observeNotificationBadge(runtime.entry.id);
        }
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
        // The §3.4 hydration barrier must FAIL CLOSED on host unhealth: a 500ing
        // host must not resolve the barrier, validate the cursor, and let the badge
        // read Connected over an empty board. `refetchType: "all"` because a
        // snapshot is not "taken" if the inactive queries the next render will read
        // were left stale.
        //
        // But not every rejection is unhealth. Queries whose commands the remote
        // facade deliberately refuses reject with `REMOTE_COMMAND_UNAVAILABLE` on
        // EVERY attempt — a capability boundary, not a health signal — and a
        // `throwOnError` sweep made those fail the barrier, which made connecting
        // structurally impossible while any such query was mounted (the supervisor
        // redialed forever over a perfectly healthy socket). So: sweep without
        // `throwOnError`, then read the settled cache and fail the barrier on any
        // error EXCEPT that one code.
        const client = getQueryClient(environmentId);
        try {
          await client.invalidateQueries({ refetchType: "all" });
        } catch (reason: unknown) {
          recordConnectionEvent(
            environmentId,
            "barrier",
            "Hydration refetch sweep failed before any query settled.",
            describeError(reason)
          );
          throw reason;
        }
        const errored = client
          .getQueryCache()
          .getAll()
          .filter((query) => query.state.error !== null);
        const isTolerated = (query: (typeof errored)[number]): boolean =>
          query.state.error instanceof RemoteTransportError &&
          query.state.error.code === "REMOTE_COMMAND_UNAVAILABLE";
        const tolerated = errored.filter(isTolerated);
        const fatal = errored.filter((query) => !isTolerated(query));
        if (tolerated.length > 0) {
          recordConnectionEvent(
            environmentId,
            "info",
            `${tolerated.length} data source${tolerated.length === 1 ? " is" : "s are"} not supported by this host (tolerated).`,
            tolerated
              .slice(0, 5)
              .map((query) => JSON.stringify(query.queryKey))
              .join(", ")
          );
        }
        const [firstFatal] = fatal;
        if (firstFatal !== undefined) {
          // Name every blocker (bounded): "which query keeps this environment amber"
          // is exactly the question the journal exists to answer.
          for (const query of fatal.slice(0, 5)) {
            recordConnectionEvent(
              environmentId,
              "barrier",
              `Hydration failed for ${JSON.stringify(query.queryKey)} — the connection stays read-only until this read succeeds.`,
              describeError(query.state.error)
            );
          }
          if (fatal.length > 5) {
            recordConnectionEvent(
              environmentId,
              "barrier",
              `…and ${fatal.length - 5} more failing queries.`
            );
          }
          throw firstFatal.state.error;
        }
      },
      sweep: () => {
        void getQueryClient(environmentId).invalidateQueries();
        // P-21 (2.7-c): ordering is replay → sweep → gate reconcile. Permission and
        // question gates live in host MEMORY, not the event log, so replaying the
        // stream cannot discover a gate raised — or resolved — while we were away.
        requestPendingGateReconcile(environmentId);
      },
      onRestartRequired: (cause) => {
        // Resolved at call time, never captured: the bus outlives any single runtime.
        const runtime = runtimes.get(environmentId);
        if (runtime === undefined) {
          return;
        }
        recordConnectionEvent(
          environmentId,
          "stream",
          cause.kind === "reset"
            ? `Host reset the event stream (${cause.reason}).`
            : cause.kind === "stream_error"
              ? `Host answered the event stream with an error (${cause.code}).`
              : cause.kind === "sequence_hole"
                ? `Event stream skipped ahead (expected ${cause.expected}, received ${cause.received}); resynchronizing.`
                : cause.kind === "protocol_error"
                  ? `Event stream protocol error: ${cause.detail}`
                  : `Event stream needs a restart (${cause.kind}).`
        );
        // `revoked` / `host_disabled` are the host WITHDRAWING the session. Routing
        // them to `streamLost` would redial the 16 s ladder forever against a host
        // that already refused this device, instead of showing the re-pair state.
        if (cause.kind === "reset" && isAuthorityResetReason(cause.reason)) {
          runtime.supervisor.authorityWithdrawn(
            `The host ended this device's session (${cause.reason}). Re-pair this environment to reconnect.`
          );
          return;
        }
        // `error{REMOTE_UNAUTHORIZED}` on an established stream is how the host actually
        // reports a revocation — it never sends `reset(revoked)` on the wire for it. The
        // ladder would cost a full backoff cycle before the ws-ticket 401 parked the
        // device, so verify once, immediately. Blanket-parking here would be wrong: the
        // same rejection covers an agent-control NARROWING on a device that stays paired.
        if (cause.kind === "stream_error" && cause.code === "REMOTE_UNAUTHORIZED") {
          runtime.supervisor.verifyAuthorityNow();
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
    // Forward declaration: the supervisor callbacks below close over `runtime`, and `runtime`
    // needs the supervisor, so the binding must exist before it can be initialised. Assigned
    // exactly once, after the supervisor is constructed and before any callback can run.
    // eslint-disable-next-line prefer-const
    let runtime: RuntimeEntry;
    const environmentId = entry.id;
    let lastJournaledPair: string | null = null;
    const supervisor = new ConnectionSupervisor({
      environmentId,
      expectedHostEnvironmentId: entry.remote?.environmentId ?? environmentId,
      clientProtocolVersion: CLIENT_PROTOCOL_VERSION,
      clientMinProtocol: CLIENT_MIN_PROTOCOL,
      fetchDescriptor: async () => {
        try {
          const response = await networkFetch(environmentId, DESCRIPTOR_PATH);
          if (!response.ok) {
            throw new Error(`Descriptor request failed with HTTP ${response.status}.`);
          }
          return parseDescriptor(await response.json());
        } catch (reason: unknown) {
          recordConnectionEvent(
            environmentId,
            "attempt",
            "Identity check failed — the host did not answer its descriptor.",
            describeError(reason)
          );
          throw reason;
        }
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
          const typed = toRemoteTransportError(reason, environmentId, "remote_connect");
          recordConnectionEvent(
            environmentId,
            "attempt",
            "Opening the event stream failed.",
            describeError(typed)
          );
          throw typed;
        }
        const outcome = parseConnectOutcome(raw);
        runtime.socketLive = true;
        recordConnectionEvent(
          environmentId,
          "info",
          "Event stream socket opened; hydrating from the host."
        );
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
        try {
          const response = await networkFetch(environmentId, HEALTH_PATH);
          if (!response.ok) {
            throw new Error(`Health probe failed with HTTP ${response.status}.`);
          }
        } catch (reason: unknown) {
          recordConnectionEvent(
            environmentId,
            "attempt",
            "Health probe failed.",
            describeError(reason)
          );
          throw reason;
        }
      },
      refreshScopes: async () => {
        if (useEnvironmentStore.getState().activeEnvironmentId !== environmentId) {
          // P-28 BACKGROUND HALF (PR 3.3): a background environment reconnects
          // without re-reading `GET /remote/v1/session`, and it must NEVER widen.
          //
          // It cannot re-read: `authorize_proxy_target` authorizes only the health ops
          // (descriptor + health) for a non-active environment (P-26,
          // `remote_environment_service.rs`). Session introspection is not a health op,
          // so asking would be denied — and widening the proxy to permit it is exactly
          // the erosion this slice is forbidden to cause.
          //
          // So the refresh NARROWS to what was last CONFIRMED, and to the empty set
          // when nothing ever was. Falling back to `entry.remote.scopes` — the
          // pairing-time snapshot — was the fail-open hole: that snapshot records what
          // the host granted when the environment was added and can be arbitrarily
          // stale, so a device whose `ui:agent` grant was revoked would have had it
          // restored by a background reconnect. An empty set fails closed; the real
          // grant is re-confirmed by the full attempt that activation forces.
          return getConfirmedScopes(environmentId) ?? [];
        }
        try {
          const response = await networkFetch(environmentId, SESSION_PATH);
          if (!response.ok) {
            throw new Error(
              `Session introspection failed with HTTP ${response.status}.`
            );
          }
          return parseScopes(await response.json());
        } catch (reason: unknown) {
          recordConnectionEvent(
            environmentId,
            "attempt",
            "Reading this device's granted permissions failed.",
            describeError(reason)
          );
          throw reason;
        }
      },
      applyScopes: (scopes) => {
        recordConnectionEvent(
          environmentId,
          "info",
          scopes.length > 0
            ? `Host confirmed this device's permissions: ${scopes.join(", ")}.`
            : "Host confirmed no permissions for this device yet."
        );
        useEnvironmentStore.getState().setEffectiveScopes(environmentId, scopes);
      },
      beginStream: async (outcome) => {
        if (useEnvironmentStore.getState().activeEnvironmentId !== environmentId) {
          return;
        }
        await runtime.bus.beginStream(outcome);
      },
      hasLiveSocket: () => runtime.socketLive,
      onStateChange: (state, presentation) => {
        publishConnectionState(environmentId, state);
        // Read `blocked()` HERE rather than wiring `onBlocked` separately: the
        // supervisor populates its blocked reason before it dispatches, so by the time
        // this callback runs the pair is already consistent. Two callbacks writing the
        // same slice would be two writers for one UI concern, and the interleaving
        // (`onBlocked` fires BEFORE the transition) would let a reader observe a
        // blocked message under a still-connecting presentation.
        const blocked = supervisor.blocked();
        // Presentation can now repaint without an FSM edge (syncing escalation), so
        // dedupe journal lines on the pair — a repaint of the same pair is not news.
        const journalPair = `${state}|${presentation}`;
        if (journalPair !== lastJournaledPair) {
          lastJournaledPair = journalPair;
          recordConnectionEvent(
            environmentId,
            "state",
            `Connection state: ${state} (shown as ${presentation}).`,
            blocked === null
              ? undefined
              : `${blocked.failure}: ${blocked.message}`
          );
        }
        useEnvironmentStore.getState().setConnectionPresentation(environmentId, {
          presentation,
          blockedFailure: blocked?.failure ?? null,
          blockedMessage: blocked?.message ?? null,
        });
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
    // The observed tally has done its job the moment this environment starts
    // projecting: the cold hydrate above re-reads real notification state, so keeping
    // the count would render it a second time on top of the hydrated truth.
    useEnvironmentStore.getState().clearNotificationBadge(environmentId);
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
      // A stopped supervisor reports nothing further, so a retained presentation would
      // be a claim about a connection nobody is maintaining.
      useEnvironmentStore.getState().clearConnectionPresentation(environmentId);
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
        useEnvironmentStore.getState().clearConnectionPresentation(environmentId);
        removeQueryClient(environmentId);
        // Persisted slices are env-scoped by row id, so leaving them behind means a host
        // that later re-pairs onto the same id inherits the removed environment's UI state
        // (`env-scoped-storage.ts`). The staged-remove command clears them on the path it
        // owns; a removal completed by the Rust startup reconciler arrives only here.
        clearEnvScopedStorage(environmentId);
        clearConnectionJournal(environmentId);
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
  runtimeActions = {
    retryNow: (environmentId) => {
      const runtime = runtimes.get(environmentId);
      if (runtime === undefined) {
        return;
      }
      recordConnectionEvent(environmentId, "action", "Retry requested by the user.");
      runtime.supervisor.retryNow();
    },
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
    runtimeActions = null;
    activeTeardown = null;
  };
  activeTeardown = teardown;
  return teardown;
}
