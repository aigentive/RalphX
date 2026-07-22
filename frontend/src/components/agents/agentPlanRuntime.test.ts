import { describe, expect, it } from "vitest";

import { materializeWorkspaceRuntimeSelection } from "./agentPlanRuntime";

describe("materializeWorkspaceRuntimeSelection", () => {
  it("materializes provider defaults consistently for every Plan CTA host", () => {
    expect(
      materializeWorkspaceRuntimeSelection(
        {
          provider: "codex",
          model: null,
          effort: null,
          serviceTier: "provider_default",
          coordinationMode: "solo",
          personaId: null,
        },
        {
          claude: [],
          codex: [
            {
              id: "gpt-5.6",
              label: "GPT-5.6",
              menuLabel: "GPT-5.6",
              defaultEffort: "high",
              supportedEfforts: ["medium", "high"],
            },
          ],
        },
      ),
    ).toEqual({ provider: "codex", modelId: "gpt-5.6-sol", effort: "medium" });
  });
});
