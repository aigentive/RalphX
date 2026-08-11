/**
 * Mock implementation of @tauri-apps/api/event for web mode
 *
 * Provides no-op event listeners that don't receive real events.
 * The event bus should use MockEventBus in web mode instead.
 */

/**
 * Event payload type (matches Tauri's EventCallback)
 */
export interface Event<T> {
  /** Event name */
  event: string;
  /** Event id */
  id: number;
  /** Event payload */
  payload: T;
}

/**
 * Unlisten function type
 */
export type UnlistenFn = () => void;

/**
 * Event callback type
 */
export type EventCallback<T> = (event: Event<T>) => void;

/**
 * Options for listen()
 */
export interface Options {
  /** Event target (window label or object) */
  target?:
    | string
    | {
        kind: "Any" | "AnyLabel" | "App" | "Window" | "Webview" | "WebviewWindow";
        label?: string;
      };
}

// Track listeners for potential debugging
const listeners = new Map<string, Set<EventCallback<unknown>>>();

/**
 * Mock listen - registers a callback for web-mode emit()
 */
export async function listen<T>(
  event: string,
  handler: EventCallback<T>,
  _options?: Options
): Promise<UnlistenFn> {
  console.debug(`[mock] listen("${event}") registered`);

  // Track the listener
  if (!listeners.has(event)) {
    listeners.set(event, new Set());
  }
  listeners.get(event)!.add(handler as EventCallback<unknown>);

  // Return unlisten function
  return () => {
    console.debug(`[mock] unlisten("${event}")`);
    listeners.get(event)?.delete(handler as EventCallback<unknown>);
  };
}

/**
 * Mock once - like listen but for a single event
 */
export async function once<T>(
  event: string,
  handler: EventCallback<T>,
  options?: Options
): Promise<UnlistenFn> {
  console.debug(`[mock] once("${event}") registered`);
  return listen(event, handler, options);
}

/**
 * Mock emit - delivers to registered web-mode listeners.
 */
export async function emit(event: string, payload?: unknown): Promise<void> {
  console.debug(`[mock] emit("${event}", ${JSON.stringify(payload)})`);
  const mockEvent: Event<unknown> = {
    event,
    id: 0,
    payload,
  };
  for (const listener of listeners.get(event) ?? []) {
    listener(mockEvent);
  }
}

/**
 * Mock emitTo - logs but doesn't actually emit
 */
export async function emitTo(
  target: string | { kind: string; label?: string },
  event: string,
  payload?: unknown
): Promise<void> {
  const targetStr = typeof target === "string" ? target : JSON.stringify(target);
  console.debug(`[mock] emitTo(${targetStr}, "${event}", ${JSON.stringify(payload)})`);
}

/**
 * TauriEvent enum - event names used by Tauri
 */
export const TauriEvent = {
  WINDOW_RESIZED: "tauri://resize",
  WINDOW_MOVED: "tauri://move",
  WINDOW_CLOSE_REQUESTED: "tauri://close-requested",
  MENU: "tauri://menu",
  WINDOW_CREATED: "tauri://window-created",
  WEBVIEW_CREATED: "tauri://webview-created",
  FILE_DROP: "tauri://file-drop",
  FILE_DROP_HOVER: "tauri://file-drop-hover",
  FILE_DROP_CANCELLED: "tauri://file-drop-cancelled",
  DRAG: "tauri://drag",
  DROP: "tauri://drop",
  DROP_CANCELLED: "tauri://drop-cancelled",
  DROP_OVER: "tauri://drop-over",
} as const;

if (typeof window !== "undefined") {
  (
    window as Window & {
      __mockTauriEmit?: (event: string, payload?: unknown) => Promise<void>;
    }
  ).__mockTauriEmit = emit;
}
