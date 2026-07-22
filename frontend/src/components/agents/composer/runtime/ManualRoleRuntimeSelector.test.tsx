import { describe, expect, it } from "vitest";

import type { ManualRoleCatalogEntry } from "@/api/manual-role-defaults.types";

import { getManualRoleRuntimeSelectionIssue } from "./manualRoleRuntimeValidation";

const entry = {
  role: "workspace_edit",
  displayName: "Edit",
  description: "Implements in a workspace.",
  family: "workspace",
  familyDisplayName: "Workspace",
  requiresTasks: false,
  configured: null,
  effective: null,
  source: null,
  diagnostics: [],
  controls: {
    capabilities: [
      { value: "solo", enabled: true, disabledReason: null },
    ],
    speeds: [
      { value: "provider_default", enabled: true, disabledReason: null },
      { value: "standard", enabled: true, disabledReason: null },
    ],
    persona: { enabled: false, disabledReason: null },
  },
} satisfies ManualRoleCatalogEntry;

const selection = {
  provider: "codex",
  model: "gpt-5.6",
  effort: "high",
  serviceTier: "standard" as const,
  coordinationMode: "solo",
  personaId: null,
};

const models = [
  {
    id: "gpt-5.6",
    label: "GPT-5.6",
    menuLabel: "GPT-5.6",
    defaultEffort: "high",
    supportedEfforts: ["medium", "high"],
  },
];

describe("getManualRoleRuntimeSelectionIssue", () => {
  it("rejects a disabled provider while leaving the stale selection inspectable", () => {
    expect(
      getManualRoleRuntimeSelectionIssue({
        entry,
        value: selection,
        providerOptions: [
          {
            id: "codex",
            label: "Codex",
            disabled: true,
            disabledReason: "Enable in Settings.",
          },
        ],
        modelsForProvider: () => models,
        personas: [],
      }),
    ).toBe("Enable in Settings.");
  });

  it("rejects removed models and unsupported effort values", () => {
    const providerOptions = [{ id: "codex", label: "Codex" }];
    expect(
      getManualRoleRuntimeSelectionIssue({
        entry,
        value: { ...selection, model: "gpt-5.5-removed" },
        providerOptions,
        modelsForProvider: () => models,
        personas: [],
      }),
    ).toMatch(/model is no longer available/i);
    expect(
      getManualRoleRuntimeSelectionIssue({
        entry,
        value: { ...selection, effort: "xhigh" },
        providerOptions,
        modelsForProvider: () => models,
        personas: [],
      }),
    ).toMatch(/effort is not supported/i);
  });

  it("accepts a current complete selection", () => {
    expect(
      getManualRoleRuntimeSelectionIssue({
        entry,
        value: selection,
        providerOptions: [{ id: "codex", label: "Codex" }],
        modelsForProvider: () => models,
        personas: [],
      }),
    ).toBeNull();
  });
});
