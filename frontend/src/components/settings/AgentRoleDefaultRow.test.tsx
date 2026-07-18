import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type {
  ManualRoleCatalogEntry,
  ManualRoleDefault,
} from "@/api/manual-role-defaults.types";
import type { Persona } from "@/types/persona";

import { AgentRoleDefaultRow } from "./AgentRoleDefaultRow";

const configured: ManualRoleDefault = {
  provider: "claude",
  model: "sonnet",
  effort: "high",
  serviceTier: "standard",
  coordinationMode: "solo",
  personaId: null,
  approvalPolicy: null,
  sandboxMode: null,
};

const entry: ManualRoleCatalogEntry = {
  role: "workspace_project",
  displayName: "Workspace Project",
  family: "workspace",
  familyDisplayName: "Workspace",
  configured,
  effective: configured,
  source: "project_ui",
  diagnostics: ["The configured model is no longer available"],
  controls: {
    capabilities: [
      { value: "solo", enabled: true, disabledReason: null },
      { value: "rx_native_team", enabled: true, disabledReason: null },
      {
        value: "rx_native_workflow",
        enabled: false,
        disabledReason: "Workflow is unavailable for this CLI version",
      },
    ],
    speeds: [
      { value: "provider_default", enabled: true, disabledReason: null },
      { value: "standard", enabled: true, disabledReason: null },
      { value: "fast", enabled: true, disabledReason: null },
    ],
    persona: { enabled: true, disabledReason: null },
  },
};

const persona: Persona = {
  id: "persona-1",
  slug: "reviewer",
  name: "Reviewer",
  description: "Reviews changes",
  content: "Review carefully",
  status: "active",
  version: 1,
  contentHash: "hash-1",
  createdAt: "2026-07-16T12:00:00Z",
  updatedAt: "2026-07-16T12:00:00Z",
};

function renderRow() {
  const onUpdate = vi.fn();
  const onFollow = vi.fn();
  const onManagePersonas = vi.fn();
  render(
    <AgentRoleDefaultRow
      entry={entry}
      disabled={false}
      providers={["claude", "codex"]}
      modelsForProvider={(provider) => provider === "claude"
        ? [{
            id: "sonnet",
            label: "Sonnet",
            menuLabel: "Sonnet",
            defaultEffort: "high",
            supportedEfforts: ["medium", "high"],
          }]
        : [{
            id: "gpt-5.6",
            label: "GPT-5.6",
            menuLabel: "GPT-5.6",
            defaultEffort: "xhigh",
            supportedEfforts: ["high", "xhigh"],
          }]}
      personas={[persona]}
      onUpdate={onUpdate}
      onFollow={onFollow}
      onManagePersonas={onManagePersonas}
    />,
  );
  return { onUpdate, onFollow };
}

describe("AgentRoleDefaultRow", () => {
  it("labels bridged legacy workspace-review defaults", () => {
    render(
      <AgentRoleDefaultRow
        entry={{ ...entry, source: "legacy_workspace_review" }}
        disabled={false}
        providers={["codex"]}
        modelsForProvider={() => []}
        personas={[]}
        onUpdate={vi.fn()}
        onFollow={vi.fn()}
        onManagePersonas={vi.fn()}
      />,
    );

    expect(
      screen.getByText("Manual default · Legacy Workspace Review"),
    ).toBeInTheDocument();
  });

  it("shows diagnostics and persists every editable runtime field", async () => {
    const user = userEvent.setup();
    const { onUpdate } = renderRow();

    expect(screen.getByRole("alert")).toHaveTextContent(
      "The configured model is no longer available",
    );
    expect(screen.getByText("Workflow is unavailable for this CLI version"))
      .toBeInTheDocument();

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace Project model" }),
      "__default__",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace Project effort" }),
      "medium",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace Project capability" }),
      "rx_native_team",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace Project speed" }),
      "fast",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace Project persona" }),
      "persona-1",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace Project approval policy" }),
      "never",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Workspace Project sandbox mode" }),
      "workspace-write",
    );

    expect(onUpdate).toHaveBeenNthCalledWith(1, {
      ...configured,
      model: null,
      effort: "high",
    });
    expect(onUpdate).toHaveBeenNthCalledWith(2, {
      ...configured,
      effort: "medium",
    });
    expect(onUpdate).toHaveBeenNthCalledWith(3, {
      ...configured,
      coordinationMode: "rx_native_team",
    });
    expect(onUpdate).toHaveBeenNthCalledWith(4, {
      ...configured,
      serviceTier: "fast",
    });
    expect(onUpdate).toHaveBeenNthCalledWith(5, {
      ...configured,
      personaId: "persona-1",
    });
    expect(onUpdate).toHaveBeenNthCalledWith(6, {
      ...configured,
      approvalPolicy: "never",
    });
    expect(onUpdate).toHaveBeenNthCalledWith(7, {
      ...configured,
      sandboxMode: "workspace-write",
    });
  });

  it("follows the inherited value from the configured row", async () => {
    const user = userEvent.setup();
    const { onFollow } = renderRow();

    await user.click(screen.getByRole("button", {
      name: "Follow Workspace Project default",
    }));

    expect(onFollow).toHaveBeenCalledOnce();
  });
});
