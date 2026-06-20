import { beforeEach, describe, expect, it } from "vitest";

import { useTicketingStore } from "./ticketingStore";

describe("useTicketingStore", () => {
  beforeEach(() => {
    useTicketingStore.getState().reset();
  });

  it("clears container and selected ticket when provider changes", () => {
    useTicketingStore.getState().setProvider("jira");
    useTicketingStore.getState().setContainerId("board-1");
    useTicketingStore.getState().setSelectedTicketRef({
      provider: "jira",
      id: "10001",
      key: "RX-1",
    });

    useTicketingStore.getState().setProvider("linear");

    expect(useTicketingStore.getState().activeProvider).toBe("linear");
    expect(useTicketingStore.getState().activeContainerId).toBeNull();
    expect(useTicketingStore.getState().selectedTicketRef).toBeNull();
  });

  it("merges filter updates and resets them independently", () => {
    useTicketingStore.getState().setFilters({
      text: "race",
      stateIds: ["started"],
    });
    useTicketingStore.getState().setFilters({ assignee: "me" });

    expect(useTicketingStore.getState().filters).toEqual({
      text: "race",
      assignee: "me",
      stateIds: ["started"],
      labels: [],
    });

    useTicketingStore.getState().resetFilters();

    expect(useTicketingStore.getState().filters).toEqual({
      text: "",
      assignee: null,
      stateIds: [],
      labels: [],
    });
  });
});
