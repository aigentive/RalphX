/**
 * Per-environment connection diagnostics journal (client-side only).
 *
 * A bounded ring buffer of human-readable connection lifecycle events — supervisor
 * state changes, attempt-step failures, hydration-barrier verdicts, stream resets —
 * written exclusively by the environment runtime composition root
 * (`lib/remote/environment-runtime.ts`) and read by the connection log viewer.
 * Nothing here talks to a host: the journal exists precisely for the moments when
 * the host cannot be reached, so the user can see WHY, not just a banner.
 */

import { create } from "zustand";

export type ConnectionJournalKind =
  /** Supervisor state transitions (connecting, connected, blocked, …). */
  | "state"
  /** A single connect-attempt step failed (descriptor, socket, scopes, probe). */
  | "attempt"
  /** Hydration-barrier verdicts: the query errors that keep a socket from going green. */
  | "barrier"
  /** Host-initiated stream teardown causes (resets, stream errors, sequence holes). */
  | "stream"
  /** Non-failure context: confirmed scopes, tolerated capability gaps. */
  | "info"
  /** User-driven wakeups (retry clicks, activation). */
  | "action";

export interface ConnectionJournalEntry {
  /** Full RFC3339 timestamp — entries must stay orderable across days. */
  readonly at: string;
  readonly kind: ConnectionJournalKind;
  readonly message: string;
  readonly detail?: string;
}

export const CONNECTION_JOURNAL_CAP = 200;

interface RemoteConnectionJournalState {
  journals: Record<string, readonly ConnectionJournalEntry[]>;
  record: (
    environmentId: string,
    kind: ConnectionJournalKind,
    message: string,
    detail?: string
  ) => void;
  clear: (environmentId: string) => void;
}

export const useRemoteConnectionJournalStore =
  create<RemoteConnectionJournalState>()((set) => ({
    journals: {},
    record: (environmentId, kind, message, detail) =>
      set((state) => {
        const existing = state.journals[environmentId] ?? [];
        const entry: ConnectionJournalEntry = {
          at: new Date().toISOString(),
          kind,
          message,
          ...(detail !== undefined && { detail }),
        };
        const overflow = existing.length + 1 - CONNECTION_JOURNAL_CAP;
        const next =
          overflow > 0
            ? [...existing.slice(overflow), entry]
            : [...existing, entry];
        return { journals: { ...state.journals, [environmentId]: next } };
      }),
    clear: (environmentId) =>
      set((state) => {
        if (!(environmentId in state.journals)) {
          return state;
        }
        const journals = { ...state.journals };
        delete journals[environmentId];
        return { journals };
      }),
  }));

/** Imperative writer for the non-React composition root. */
export function recordConnectionEvent(
  environmentId: string,
  kind: ConnectionJournalKind,
  message: string,
  detail?: string
): void {
  useRemoteConnectionJournalStore
    .getState()
    .record(environmentId, kind, message, detail);
}

/** Drops an environment's journal — called when its registry row is removed. */
export function clearConnectionJournal(environmentId: string): void {
  useRemoteConnectionJournalStore.getState().clear(environmentId);
}
