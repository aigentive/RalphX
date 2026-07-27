import { describe, expect, it } from "vitest";

import { buildCapabilityOptions } from "./capabilityOptions";

describe("buildCapabilityOptions", () => {
  it.each([
    {
      flags: {
        teamEnabled: false,
        workflowsEnabled: false,
        codexUltraAvailable: false,
      },
      ids: ["solo"],
    },
    {
      flags: {
        teamEnabled: true,
        workflowsEnabled: false,
        codexUltraAvailable: false,
      },
      ids: ["solo", "rx_native_team"],
    },
    {
      flags: {
        teamEnabled: false,
        workflowsEnabled: true,
        codexUltraAvailable: false,
      },
      ids: ["solo", "rx_native_workflow"],
    },
    {
      flags: {
        teamEnabled: false,
        workflowsEnabled: false,
        codexUltraAvailable: true,
      },
      ids: ["solo", "codex_native_ultra"],
    },
    {
      flags: {
        teamEnabled: true,
        workflowsEnabled: true,
        codexUltraAvailable: false,
      },
      ids: ["solo", "rx_native_team", "rx_native_workflow"],
    },
    {
      flags: {
        teamEnabled: true,
        workflowsEnabled: false,
        codexUltraAvailable: true,
      },
      ids: ["solo", "rx_native_team", "codex_native_ultra"],
    },
    {
      flags: {
        teamEnabled: false,
        workflowsEnabled: true,
        codexUltraAvailable: true,
      },
      ids: ["solo", "rx_native_workflow", "codex_native_ultra"],
    },
    {
      flags: {
        teamEnabled: true,
        workflowsEnabled: true,
        codexUltraAvailable: true,
      },
      ids: [
        "solo",
        "rx_native_team",
        "rx_native_workflow",
        "codex_native_ultra",
      ],
    },
  ])("returns the enabled options in established order", ({ flags, ids }) => {
    expect(buildCapabilityOptions(flags).map((option) => option.id)).toEqual(ids);
  });

  it("keeps the non-Team descriptions unchanged and makes Team model-directed", () => {
    const options = buildCapabilityOptions({
      teamEnabled: true,
      workflowsEnabled: true,
      codexUltraAvailable: true,
    });

    expect(options).toEqual([
      {
        id: "solo",
        label: "Defaults",
        description: "Use the selected provider without extra orchestration.",
      },
      {
        id: "rx_native_team",
        label: "Team",
        description:
          "Let this agent delegate to RalphX teammates when it helps; it may also work alone.",
      },
      {
        id: "rx_native_workflow",
        label: "Workflow",
        description: "Generate and run a durable reviewed orchestration script.",
      },
      {
        id: "codex_native_ultra",
        label: "Ultra",
        description: "Activate Codex provider-native subagents and maximum reasoning.",
      },
    ]);
  });
});
