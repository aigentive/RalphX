import { describe, expect, it } from "vitest";

import {
  MCP_SETUP_PREFLIGHT_MARKER,
  LINKED_SETUP_FAILURE_MARKER,
  buildAgentStartConversationRetryInput,
  parseMcpSetupPreflightFailure,
  parseLinkedSetupFailure,
} from "./agentStartErrors";

describe("agentStartErrors", () => {
  it("parses only linked setup failure messages", () => {
    expect(parseLinkedSetupFailure(new Error("plain failure"))).toBeNull();
    expect(parseLinkedSetupFailure({ message: LINKED_SETUP_FAILURE_MARKER })).toBeNull();

    expect(
      parseLinkedSetupFailure(
        new Error(`${LINKED_SETUP_FAILURE_MARKER} Selected branch failed`),
      ),
    ).toEqual({ message: "Selected branch failed" });

    expect(parseLinkedSetupFailure(LINKED_SETUP_FAILURE_MARKER)).toEqual({
      message: "Linked branch setup failed.",
    });
  });

  it("preserves optional retry input fields explicitly supplied by the caller", () => {
    expect(
      buildAgentStartConversationRetryInput({
        projectId: "project-1",
        content: "retry this",
        runtime: {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
        runtimeProviderContext: {
          supportedEfforts: ["low", "medium", "high", "xhigh"],
          supportedModelAliases: ["gpt-5.5"],
        },
        mode: "edit",
        base: {
          kind: "local_branch",
          ref: "feature/retry",
          displayName: "feature/retry",
          branchMode: "linked",
        },
        codexFastMode: true,
        personaId: null,
        capabilityIntent: { coordinationMode: "rx_native_workflow" },
        teamIntent: { coordinationMode: "rx_native_team" },
        composerArtifactReferences: [
          {
            kind: "plan",
            artifactId: "artifact-1",
            title: "Plan",
          },
        ],
        composerIntegrationReferences: [
          {
            provider: "atlassian",
            kind: "jira",
            id: "RX-1",
            key: "RX-1",
          },
        ],
        composerProjectReferences: [{ kind: "file", path: "src/main.ts" }],
      }),
    ).toEqual({
      projectId: "project-1",
      content: "retry this",
      runtime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
      runtimeProviderContext: {
        supportedEfforts: ["low", "medium", "high", "xhigh"],
        supportedModelAliases: ["gpt-5.5"],
      },
      mode: "edit",
      base: {
        kind: "local_branch",
        ref: "feature/retry",
        displayName: "feature/retry",
        branchMode: "linked",
      },
      codexFastMode: true,
      personaId: null,
      capabilityIntent: { coordinationMode: "rx_native_workflow" },
      teamIntent: { coordinationMode: "rx_native_team" },
      composerArtifactReferences: [
        {
          kind: "plan",
          artifactId: "artifact-1",
          title: "Plan",
        },
      ],
      composerIntegrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-1",
          key: "RX-1",
        },
      ],
      composerProjectReferences: [{ kind: "file", path: "src/main.ts" }],
    });
  });

  it("parses a redacted MCP setup payload without depending on prose", () => {
    expect(parseMcpSetupPreflightFailure(new Error("plain failure"))).toBeNull();
    expect(
      parseMcpSetupPreflightFailure(
        new Error(
          `${MCP_SETUP_PREFLIGHT_MARKER}{"provider":"claude","server_id":"ralphx","scope":"user","conflict_kind":"legacy_repair_failed","repair_status":"failed"}`,
        ),
      ),
    ).toEqual({
      provider: "claude",
      serverId: "ralphx",
      scope: "user",
      conflictKind: "legacy_repair_failed",
      repairStatus: "failed",
    });
    expect(
      parseMcpSetupPreflightFailure(
        `${MCP_SETUP_PREFLIGHT_MARKER}{"provider":"claude","server_id":"ralphx","command":"/secret"}`,
      ),
    ).toBeNull();
  });
});
