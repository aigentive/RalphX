import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import type { ManagedTeamStatus } from "@/api/managed-team";
import {
  managedTeamKeys,
  managedTeamStatusRefetchInterval,
  reconcileManagedTeamEvent,
} from "./useManagedTeam";

function status(memberGeneration = 2): ManagedTeamStatus {
  return {
    session: {
      id: "team-1",
      projectId: "project-1",
      coordinatorConversationId: "conversation-1",
      status: "active",
      configuredConcurrency: 3,
      effectiveConcurrency: 2,
      automaticWakeLimit: 4,
      version: 1,
      createdAt: "2026-07-28T10:00:00Z",
      updatedAt: "2026-07-28T10:01:00Z",
    },
    members: [
      {
        id: "member-1",
        teamId: "team-1",
        name: "Scout",
        normalizedName: "scout",
        canonicalAgentName: "ralphx-general-explorer",
        roleSummary: "Investigates focused questions.",
        status: "idle",
        generation: memberGeneration,
      },
    ],
    usage: { tokens: 0, costMicros: 0, members: [] },
  };
}

function createClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

describe("reconcileManagedTeamEvent", () => {
  it("patches the matching conversation cache and rejects stale member generations", () => {
    const client = createClient();
    client.setQueryData(managedTeamKeys.status("conversation-1"), status(2));

    expect(
      reconcileManagedTeamEvent(client, "conversation-1", "run-1", {
        conversationId: "conversation-1",
        parentRunId: "run-1",
        sequence: 10,
        member: { ...status(3).members[0], status: "working" },
      }),
    ).toBe(true);
    expect(
      client.getQueryData<ManagedTeamStatus>(
        managedTeamKeys.status("conversation-1"),
      )?.members[0]?.status,
    ).toBe("working");

    expect(
      reconcileManagedTeamEvent(client, "conversation-1", "run-1", {
        conversationId: "conversation-1",
        parentRunId: "run-1",
        sequence: 11,
        member: { ...status(1).members[0], status: "failed" },
      }),
    ).toBe(false);
    expect(
      client.getQueryData<ManagedTeamStatus>(
        managedTeamKeys.status("conversation-1"),
      )?.members[0]?.status,
    ).toBe("working");
  });

  it("rejects events from another conversation or parent run", () => {
    const client = createClient();
    client.setQueryData(managedTeamKeys.status("conversation-1"), status());

    expect(
      reconcileManagedTeamEvent(client, "conversation-1", "run-1", {
        conversationId: "conversation-2",
        parentRunId: "run-1",
        sequence: 1,
      }),
    ).toBe(false);
    expect(
      reconcileManagedTeamEvent(client, "conversation-1", "run-1", {
        conversationId: "conversation-1",
        parentRunId: "run-2",
        sequence: 1,
      }),
    ).toBe(false);
  });

  it("fails closed when active run authority is present but the event has no run id", () => {
    const client = createClient();
    client.setQueryData(managedTeamKeys.status("conversation-1"), status());

    expect(
      reconcileManagedTeamEvent(client, "conversation-1", "run-1", {
        conversationId: "conversation-1",
        sequence: 1,
      }),
    ).toBe(false);
  });

  it("accepts matching run events and events without local run authority for the active conversation", () => {
    const client = createClient();
    client.setQueryData(managedTeamKeys.status("conversation-1"), status());

    expect(
      reconcileManagedTeamEvent(client, "conversation-1", "run-1", {
        conversationId: "conversation-1",
        parentRunId: "run-1",
        sequence: 1,
      }),
    ).toBe(true);
    expect(
      reconcileManagedTeamEvent(client, "conversation-1", null, {
        conversationId: "conversation-1",
        sequence: 2,
      }),
    ).toBe(true);
    expect(
      reconcileManagedTeamEvent(client, "conversation-1", null, {
        conversationId: "conversation-2",
        sequence: 3,
      }),
    ).toBe(false);
  });
});

describe("managedTeamStatusRefetchInterval", () => {
  it("continues polling while an active Team member is in flight", () => {
    expect(managedTeamStatusRefetchInterval(status())).toBe(false);
    expect(
      managedTeamStatusRefetchInterval({
        ...status(),
        members: [{ ...status().members[0]!, status: "working" }],
      }),
    ).toBe(12_000);
  });

  it("stops polling once no Team members are in flight", () => {
    expect(
      managedTeamStatusRefetchInterval({
        ...status(),
        members: [{ ...status().members[0]!, status: "idle" }],
      }),
    ).toBe(false);
  });
});
