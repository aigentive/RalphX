export type ChatScrollTraceSource = "controller" | "layout" | "manual";

export interface ChatScrollTraceInput {
  conversationId: string | null;
  source: ChatScrollTraceSource;
  event: string;
  state?: string | null;
  element: HTMLElement | null;
  detail?: Record<string, unknown>;
}

export interface ChatScrollTraceEntry {
  sequence: number;
  timestampMs: number;
  conversationId: string | null;
  source: ChatScrollTraceSource;
  event: string;
  state: string | null;
  geometry: {
    scrollTop: number;
    scrollHeight: number;
    clientHeight: number;
    trueBottom: number;
    distanceToBottom: number;
  } | null;
  detail: Record<string, unknown>;
}

export interface ChatScrollDiagnostics {
  start(conversationId?: string): void;
  stop(): ChatScrollTraceEntry[];
  clear(): void;
  read(): ChatScrollTraceEntry[];
  copy(): Promise<number>;
  mark(label: string): void;
  record(input: ChatScrollTraceInput): void;
}

interface ChatScrollDiagnosticsOptions {
  copyText(text: string): Promise<void>;
  now(): number;
  maxEvents?: number;
}

const DEFAULT_MAX_EVENTS = 2_000;

export function createChatScrollDiagnostics({
  copyText,
  now,
  maxEvents = DEFAULT_MAX_EVENTS,
}: ChatScrollDiagnosticsOptions): ChatScrollDiagnostics {
  let isRecording = false;
  let conversationFilter: string | null = null;
  let sequence = 0;
  let entries: ChatScrollTraceEntry[] = [];

  const read = (): ChatScrollTraceEntry[] => entries.map((entry) => ({
    ...entry,
    geometry: entry.geometry ? { ...entry.geometry } : null,
    detail: { ...entry.detail },
  }));

  const record = (input: ChatScrollTraceInput): void => {
    if (
      !isRecording
      || (conversationFilter !== null && input.conversationId !== conversationFilter)
    ) {
      return;
    }
    const trueBottom = input.element
      ? Math.max(0, input.element.scrollHeight - input.element.clientHeight)
      : null;
    sequence += 1;
    entries.push({
      sequence,
      timestampMs: now(),
      conversationId: input.conversationId,
      source: input.source,
      event: input.event,
      state: input.state ?? null,
      geometry: input.element && trueBottom !== null
        ? {
            scrollTop: input.element.scrollTop,
            scrollHeight: input.element.scrollHeight,
            clientHeight: input.element.clientHeight,
            trueBottom,
            distanceToBottom: trueBottom - input.element.scrollTop,
          }
        : null,
      detail: { ...input.detail },
    });
    if (entries.length > maxEvents) {
      entries = entries.slice(-maxEvents);
    }
  };

  return {
    start(conversationId) {
      entries = [];
      sequence = 0;
      conversationFilter = conversationId ?? null;
      isRecording = true;
    },
    stop() {
      isRecording = false;
      return read();
    },
    clear() {
      entries = [];
      sequence = 0;
    },
    read,
    async copy() {
      const snapshot = read();
      await copyText(JSON.stringify(snapshot, null, 2));
      return snapshot.length;
    },
    mark(label) {
      record({
        conversationId: conversationFilter,
        source: "manual",
        event: `mark:${label}`,
        element: null,
      });
    },
    record,
  };
}

function getDevDiagnostics(): ChatScrollDiagnostics | null {
  if (!import.meta.env.DEV || typeof window === "undefined") return null;
  window.__chatScrollDiagnostics ??= createChatScrollDiagnostics({
    copyText: (text) => navigator.clipboard.writeText(text),
    now: () => performance.now(),
  });
  return window.__chatScrollDiagnostics;
}

export function recordChatScrollTrace(input: ChatScrollTraceInput): void {
  getDevDiagnostics()?.record(input);
}

export function installChatScrollDiagnostics(): void {
  getDevDiagnostics();
}

// Install at module evaluation so Vite Fast Refresh exposes the API even when
// it preserves an already-mounted ChatMessageList and does not rerun effects.
installChatScrollDiagnostics();
