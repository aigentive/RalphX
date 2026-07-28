/**
 * The updater updates THIS app. Its native events must never be steerable by a host
 * (PR 2.6-a, Decision 4).
 *
 * `EventProvider` is env-keyed (`EventProvider.tsx:137-144`), so `useEventBus()`
 * returns the ACTIVE environment's bus — under a remote environment that is a
 * `NetworkEventBus` fed by relayed host frames. Left unpinned, a host could emit
 * `ralphx://check-for-updates` and drive an update flow on the client.
 *
 * It is already pinned, and NOT by a second bus instance: `NetworkEventBus.subscribe`
 * routes any name in `LOCAL_ONLY_BACKEND_EVENTS` to the wrapped LOCAL bus
 * (`network-event-bus.ts:169-172`), while relayed frames dispatch to the remote
 * registry. The updater's handler therefore lives somewhere relayed frames cannot
 * reach. That is a structural pin, so the tests that protect it are (a) the negative
 * behavioural test and (b) the membership assertion — because deleting the two names
 * from the generated mirror is exactly how this silently breaks.
 */

import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { LOCAL_ONLY_BACKEND_EVENTS } from "@/lib/remote/local-only-backend-events.generated";
import { NetworkEventBus } from "@/lib/remote/network-event-bus";
import { useUpdateCheckerNativeEvents } from "@/components/UpdateChecker.events";
import type { EventBus } from "@/lib/event-bus";

// `useEventBus` is stubbed rather than mounting `EventProvider`, whose
// `GlobalEventListeners` child drags in the whole query/store tree. What is under
// test is which BUS the hook subscribes through and where relayed frames land — the
// provider's job of choosing the active env's bus is asserted in its own suite.
const busRef: { current: EventBus | null } = { current: null };
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => busRef.current,
}));

const UPDATE_CHECK_EVENT = "ralphx://check-for-updates";
const RELEASE_NOTES_EVENT = "ralphx://show-release-notes";

describe("updater native events", () => {
  it("keeps both updater events in the local-only backend mirror", () => {
    // If either name leaves this list, `NetworkEventBus.subscribe` stops delegating
    // it and the updater silently becomes host-drivable.
    expect(LOCAL_ONLY_BACKEND_EVENTS).toContain(UPDATE_CHECK_EVENT);
    expect(LOCAL_ONLY_BACKEND_EVENTS).toContain(RELEASE_NOTES_EVENT);
  });

  it("does not run an update check for an event relayed on a remote bus", () => {
    const localHandlers = new Map<string, Set<(payload: unknown) => void>>();
    const localBus: EventBus = {
      subscribe: (event: string, handler: (payload: never) => void) => {
        const set = localHandlers.get(event) ?? new Set();
        set.add(handler as (payload: unknown) => void);
        localHandlers.set(event, set);
        const unsubscribe = () =>
          set.delete(handler as (payload: unknown) => void);
        unsubscribe.ready = Promise.resolve();
        return unsubscribe;
      },
      emit: () => {},
    } as unknown as EventBus;

    const remoteBus = new NetworkEventBus({
      environmentId: "env-remote",
      localBus,
      sendFrame: async () => {},
      hydrate: async () => {},
      sweep: () => {},
      onRestartRequired: () => {},
    });

    const checkForUpdates = vi.fn();
    const openCurrentReleaseNotes = vi.fn();

    busRef.current = remoteBus as unknown as EventBus;
    renderHook(() =>
      useUpdateCheckerNativeEvents({
        checkForUpdates,
        openCurrentReleaseNotes,
      }),
    );

    // A host relaying the event onto the remote environment's own registry.
    // `emit` and applied stream frames share one dispatch path
    // (`network-event-bus.ts` `dispatch`), so this is the frame's reach.
    remoteBus.emit(UPDATE_CHECK_EVENT, {});
    remoteBus.emit(RELEASE_NOTES_EVENT, {});

    expect(checkForUpdates).not.toHaveBeenCalled();
    expect(openCurrentReleaseNotes).not.toHaveBeenCalled();

    // Positive control: the LOCAL backend emitting the same event still works, so
    // the pin is a routing decision and not a dead subscription.
    for (const handler of localHandlers.get(UPDATE_CHECK_EVENT) ?? []) {
      handler({});
    }
    expect(checkForUpdates).toHaveBeenCalledWith({ manual: true, force: true });
  });
});
