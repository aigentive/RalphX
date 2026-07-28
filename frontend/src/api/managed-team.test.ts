import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { managedTeamApi } from "./managed-team";

const fetchMock = vi.fn();

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

const sessionResponse = {
  id: "team-1",
  projectId: "project-1",
  coordinatorConversationId: "conversation-1",
  status: "active",
  configuredConcurrency: 3,
  effectiveConcurrency: 2,
  automaticWakeLimit: 4,
  version: 7,
  createdAt: "2026-07-28T10:00:00Z",
  updatedAt: "2026-07-28T10:01:00Z",
};

const memberResponse = {
  id: "member-1",
  teamId: "team-1",
  name: "Scout",
  normalizedName: "scout",
  canonicalAgentName: "ralphx-general-explorer",
  roleSummary: "Investigates focused questions.",
  status: "idle",
  generation: 2,
};

describe("managedTeamApi", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("parses the backend's camelCase status response into the public Team model", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({ session: sessionResponse, members: [memberResponse] }),
    );

    await expect(managedTeamApi.getStatus("conversation-1")).resolves.toEqual({
      session: expect.objectContaining({
        projectId: "project-1",
        coordinatorConversationId: "conversation-1",
        effectiveConcurrency: 2,
      }),
      members: [
        expect.objectContaining({
          canonicalAgentName: "ralphx-general-explorer",
          normalizedName: "scout",
        }),
      ],
    });
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining("/api/managed_team/status/conversation-1"),
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("sends ensure with camelCase input and member actions with the backend request casing", async () => {
    fetchMock
      .mockResolvedValueOnce(jsonResponse(sessionResponse))
      .mockResolvedValueOnce(jsonResponse(memberResponse));

    await managedTeamApi.ensure({
      conversationId: "conversation-1",
      projectId: "project-1",
    });
    await managedTeamApi.addMember({
      authority: { conversationId: "conversation-1", agentRunId: "run-1" },
      name: "Scout",
      canonicalAgentName: "ralphx-general-explorer",
      roleSummary: "Investigates focused questions.",
    });

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      expect.stringContaining("/api/managed_team/ensure"),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          conversationId: "conversation-1",
          projectId: "project-1",
        }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      expect.stringContaining("/api/managed_team/member"),
      expect.objectContaining({
        headers: expect.objectContaining({
          "x-ralphx-conversation-id": "conversation-1",
          "x-ralphx-agent-run-id": "run-1",
        }),
        body: JSON.stringify({
          name: "Scout",
          canonical_agent_name: "ralphx-general-explorer",
          role_summary: "Investigates focused questions.",
        }),
      }),
    );
  });
});
