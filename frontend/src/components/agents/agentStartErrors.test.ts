import { describe, expect, it } from "vitest";

import {
  LINKED_SETUP_FAILURE_MARKER,
  buildAgentStartConversationRetryInput,
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
});
