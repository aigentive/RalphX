/**
 * Event Bus Abstraction
 *
 * Provides a unified interface for event subscription that works in both:
 * - Tauri mode: Uses real Tauri listen() from @tauri-apps/api/event
 * - Web mode: Uses in-memory event emitter for browser testing
 *
 * This abstraction allows the app to run without Tauri for visual testing
 * and Playwright automation.
 *
 * TWO CONSTRAINTS THIS FILE NOW CARRIES:
 *
 * 1. This is the SOLE module allowed to import raw `listen`/`once` from
 *    @tauri-apps/api/event. Enforced in CI by
 *    scripts/check-raw-tauri-event-listen.mjs. Every other consumer goes through
 *    `useEventBus()`.
 * 2. `EventBus` is the swap seam for remote environments: a remote-mode app selects a
 *    network-backed implementation instead of `TauriEventBus`. Keep the interface
 *    transport-agnostic — no Tauri-specific semantics in the contract — and keep
 *    `Unsubscribe` non-throwing with a `ready` that always settles.
 *
 * Note for the network implementation: names classified Local-only (host chrome such as
 * the native-menu updater events) have no remote meaning. They must still reach their
 * local subscriber when a remote environment is active, so a network bus should route
 * Local-only names to a wrapped local bus rather than have chrome consumers reach around
 * `useEventBus()` — reintroducing raw `listen` would erode constraint 1.
 */

import { listen, emit, type UnlistenFn, type Event } from "@tauri-apps/api/event";
import { isTauriMode } from "./tauri-detection";

/**
 * Unsubscribe function returned by subscribe(). The function must never throw.
 *
 * `ready` resolves once the handler is registered with the underlying implementation,
 * including when registration failed. It is a REGISTRATION barrier only — never a
 * connectivity or delivery signal. A one-shot promise cannot describe a reconnecting
 * transport's lifecycle, so an implementation backed by one must resolve `ready` on local
 * registration (synchronously, like MockEventBus); gating it on "connected" would deadlock
 * every `await unsubscribe.ready` producer while the transport sits in backoff.
 */
export type Unsubscribe = (() => void) & { ready: Promise<void> };

/**
 * Event handler function
 */
export type EventHandler<T = unknown> = (payload: T) => void;

/**
 * Event bus interface for subscribing to and emitting events
 */
export interface EventBus {
  /**
   * Subscribe to an event.
   *
   * Every call is an INDEPENDENT subscription, even when passed a handler reference that
   * is already subscribed, and the returned Unsubscribe removes only that subscription.
   * (Do not de-duplicate by handler identity: the first unsubscribe would then kill a
   * sibling's live subscription.)
   *
   * @param event - Event name to listen for
   * @param handler - Callback function receiving the event payload
   * @returns Unsubscribe function to stop listening
   */
  subscribe<T = unknown>(event: string, handler: EventHandler<T>): Unsubscribe;

  /**
   * Emit an event to LOCAL subscribers only.
   *
   * Not a test-only affordance: production uses it as the in-app re-broadcast channel
   * (for example bridging `task:updated` to graph hooks, and `:local`-suffixed synthetics).
   * An implementation must never write an emit to a network transport.
   *
   * Names emitted from the webview must be classified webview-origin/Local-only — use a
   * `:local` suffix for new synthetics. Backend-classified names are reserved for the Rust
   * backend, because a host's event capture observes the Tauri bus and cannot tell a
   * webview emit from a backend one; emitting a durable backend name here would be
   * recorded and fanned out to remote clients as backend truth.
   *
   * Delivery timing is implementation-defined (Tauri is async over IPC, Mock is
   * synchronous). Do not rely on re-entrancy or same-tick delivery.
   *
   * @param event - Event name to emit
   * @param payload - Event payload data
   */
  emit<T = unknown>(event: string, payload: T): void;
}

/**
 * Tauri Event Bus - Uses real Tauri listen() API
 *
 * Wraps the Tauri event system for use in the native app.
 *
 * IMPORTANT: Events are buffered during listener setup to prevent race conditions.
 * When subscribe() is called, the Tauri listen() function returns a promise.
 * Events emitted before that promise resolves would be lost without buffering.
 * This is critical for streaming events (agent:message) that start immediately
 * after task execution begins.
 */
export class TauriEventBus implements EventBus {
  private unlisteners: Map<string, Set<Promise<UnlistenFn>>> = new Map();
  private readyListeners: Set<string> = new Set();

  subscribe<T = unknown>(event: string, handler: EventHandler<T>): Unsubscribe {
    // Generate unique ID for this subscription to track ready state
    const subscriptionId = `${event}-${Date.now()}-${Math.random()}`;
    let isUnsubscribed = false;

    // Buffer to hold events received before listener is ready
    const buffer: T[] = [];
    const deliver = (payload: T) => {
      if (isUnsubscribed) {
        return;
      }
      try {
        handler(payload);
      } catch (error) {
        console.error(`[TauriEventBus] Error in handler for ${event}:`, error);
      }
    };

    // Create the listener - buffer events until ready
    const unlistenPromise = listen<T>(event, (e: Event<T>) => {
      if (isUnsubscribed) {
        return;
      }
      if (this.readyListeners.has(subscriptionId)) {
        // Listener is ready, deliver directly
        deliver(e.payload);
      } else {
        // Listener not ready yet, buffer the event
        buffer.push(e.payload);
      }
    }).then((unlisten) => {
      if (isUnsubscribed) {
        return unlisten;
      }
      // Mark as ready and flush buffered events
      this.readyListeners.add(subscriptionId);
      for (const payload of buffer) {
        deliver(payload);
      }
      buffer.length = 0;
      return unlisten;
    }).catch((error) => {
      buffer.length = 0;
      this.readyListeners.delete(subscriptionId);
      console.warn(`[TauriEventBus] Failed to listen for ${event}:`, error);
      return () => {};
    });

    // Track for cleanup
    if (!this.unlisteners.has(event)) {
      this.unlisteners.set(event, new Set());
    }
    this.unlisteners.get(event)!.add(unlistenPromise);

    // Return unsubscribe function
    const unsubscribe = () => {
      if (isUnsubscribed) {
        return;
      }
      isUnsubscribed = true;
      buffer.length = 0;
      // IMPORTANT: Don't delete from readyListeners until unlisten actually completes.
      // Otherwise, events arriving between cleanup start and actual unlisten completion
      // will be buffered to an orphaned buffer that never gets flushed.
      this.unlisteners.get(event)?.delete(unlistenPromise);
      void unlistenPromise.then((fn) => {
        try {
          fn();
        } catch (error) {
          console.warn(`[TauriEventBus] Failed to unlisten for ${event}:`, error);
        }
      }).catch(() => {
        // listen() already logged the failure above. Swallow here so cleanup
        // never creates an unhandled rejection during component teardown.
      }).finally(() => {
        this.readyListeners.delete(subscriptionId);
      });
    };
    unsubscribe.ready = unlistenPromise.then(() => undefined);
    return unsubscribe;
  }

  emit<T = unknown>(event: string, payload: T): void {
    // Use Tauri's emit for IPC if needed
    emit(event, payload).catch((err) => {
      console.warn(`[TauriEventBus] Failed to emit ${event}:`, err);
    });
  }
}

/**
 * Mock Event Bus - In-memory event emitter for browser mode
 *
 * Used when running without Tauri (e.g., Playwright tests, dev:web mode).
 * Provides the same interface but events stay in-browser.
 */
export class MockEventBus implements EventBus {
  // Subscriptions, not handlers: storing raw handler references in a Set collapses two
  // subscriptions of the same function into one, so the first unsubscribe would kill the
  // survivor — a divergence from TauriEventBus that web mode must not have.
  private listeners: Map<string, Set<{ handler: EventHandler<unknown> }>> = new Map();

  subscribe<T = unknown>(event: string, handler: EventHandler<T>): Unsubscribe {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    // Cast needed because Map stores EventHandler<unknown>
    const subscription = { handler: handler as EventHandler<unknown> };
    this.listeners.get(event)!.add(subscription);

    // Return unsubscribe function
    const unsubscribe = () => {
      this.listeners.get(event)?.delete(subscription);
    };
    unsubscribe.ready = Promise.resolve();
    return unsubscribe;
  }

  emit<T = unknown>(event: string, payload: T): void {
    const handlers = this.listeners.get(event);
    if (handlers) {
      // Snapshot so a handler that subscribes/unsubscribes during dispatch cannot mutate
      // the set being iterated.
      [...handlers].forEach(({ handler }) => {
        try {
          handler(payload);
        } catch (err) {
          console.error(`[MockEventBus] Error in handler for ${event}:`, err);
        }
      });
    }
  }

  /**
   * Clear all listeners (useful for test cleanup)
   */
  clear(): void {
    this.listeners.clear();
  }

  /**
   * Get listener count for an event (useful for debugging/testing)
   */
  getListenerCount(event: string): number {
    return this.listeners.get(event)?.size ?? 0;
  }
}

/**
 * Create the appropriate event bus based on environment
 *
 * @returns TauriEventBus in Tauri mode, MockEventBus in browser mode
 */
export function createEventBus(): EventBus {
  if (isTauriMode()) {
    return new TauriEventBus();
  }
  return new MockEventBus();
}
