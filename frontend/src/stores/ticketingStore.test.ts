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

  it("clears container and selected ticket when switching to ClickUp", () => {
    useTicketingStore.getState().setProvider("linear");
    useTicketingStore.getState().setContainerId("team-1");
    useTicketingStore.getState().setSelectedTicketRef({
      provider: "linear",
      id: "LIN-1",
      key: "ENG-1",
    });

    useTicketingStore.getState().setProvider("clickup");

    expect(useTicketingStore.getState().activeProvider).toBe("clickup");
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

  it("records a last-opened timestamp per ticket and clears it on reset", () => {
    useTicketingStore.getState().markTicketOpened("linear:ABC-1");
    useTicketingStore.getState().markTicketOpened("jira:RX-9");

    const opened = useTicketingStore.getState().lastOpenedAt;
    expect(opened["linear:ABC-1"]).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(opened["jira:RX-9"]).toMatch(/^\d{4}-\d{2}-\d{2}T/);

    useTicketingStore.getState().reset();
    expect(useTicketingStore.getState().lastOpenedAt).toEqual({});
  });
});
