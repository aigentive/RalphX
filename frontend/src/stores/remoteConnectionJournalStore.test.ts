import { beforeEach, describe, expect, it } from "vitest";

import {
  CONNECTION_JOURNAL_CAP,
  clearConnectionJournal,
  recordConnectionEvent,
  useRemoteConnectionJournalStore,
} from "./remoteConnectionJournalStore";

beforeEach(() => {
  useRemoteConnectionJournalStore.setState({ journals: {} });
});

describe("remoteConnectionJournalStore", () => {
  it("appends entries with a full timestamp, in arrival order", () => {
    recordConnectionEvent("env-b", "state", "Connection state: connecting");
    recordConnectionEvent("env-b", "attempt", "Health probe failed.", "HTTP 500");

    const journal = useRemoteConnectionJournalStore.getState().journals["env-b"];
    expect(journal).toHaveLength(2);
    expect(journal?.[0]?.message).toBe("Connection state: connecting");
    expect(journal?.[1]?.detail).toBe("HTTP 500");
    // Full timestamps, not clock-face strings: entries must stay orderable across days.
    expect(journal?.[0]?.at).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/);
  });

  it("keeps journals isolated per environment", () => {
    recordConnectionEvent("env-b", "state", "b-entry");
    recordConnectionEvent("env-c", "state", "c-entry");

    const { journals } = useRemoteConnectionJournalStore.getState();
    expect(journals["env-b"]?.map((entry) => entry.message)).toEqual(["b-entry"]);
    expect(journals["env-c"]?.map((entry) => entry.message)).toEqual(["c-entry"]);
  });

  it("caps the journal by dropping the oldest entries", () => {
    for (let index = 0; index < CONNECTION_JOURNAL_CAP + 5; index += 1) {
      recordConnectionEvent("env-b", "state", `entry-${index}`);
    }

    const journal = useRemoteConnectionJournalStore.getState().journals["env-b"];
    expect(journal).toHaveLength(CONNECTION_JOURNAL_CAP);
    expect(journal?.[0]?.message).toBe("entry-5");
    expect(journal?.[journal.length - 1]?.message).toBe(
      `entry-${CONNECTION_JOURNAL_CAP + 4}`
    );
  });

  it("clears exactly one environment's journal", () => {
    recordConnectionEvent("env-b", "state", "b-entry");
    recordConnectionEvent("env-c", "state", "c-entry");

    clearConnectionJournal("env-b");

    const { journals } = useRemoteConnectionJournalStore.getState();
    expect(journals["env-b"]).toBeUndefined();
    expect(journals["env-c"]).toHaveLength(1);
  });
});
