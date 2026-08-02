import { describe, expect, it } from "vitest";

import { getAgentQueueHaltState } from "./agentExecutionPause";

describe("getAgentQueueHaltState", () => {
  it("suppresses a false halt banner while execution status is unknown", () => {
    expect(
      getAgentQueueHaltState({
        isKnown: false,
        isPaused: true,
        haltMode: "stopped",
      }),
    ).toBeNull();
  });

  it("preserves the known stopped presentation", () => {
    expect(
      getAgentQueueHaltState({
        isKnown: true,
        isPaused: true,
        haltMode: "stopped",
      }),
    ).toBe("stopped");
  });
});
