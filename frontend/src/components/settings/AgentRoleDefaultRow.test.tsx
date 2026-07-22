import { render, screen, waitFor } from "@testing-library/react";
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
  description: "General project conversation and workspace assistance.",
  family: "workspace",
  familyDisplayName: "Workspace",
  configured,
  effective: configured,
  source: "project_ui",
  diagnostics: [],
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

function renderRow(
  overrides: Partial<React.ComponentProps<typeof AgentRoleDefaultRow>> = {},
) {
  const onUpdate = vi.fn();
  const onExpandedChange = vi.fn();
  const onUseInheritedDefault = vi.fn().mockResolvedValue(true);
  render(
    <AgentRoleDefaultRow
      entry={entry}
      expanded={false}
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
      onExpandedChange={onExpandedChange}
      onUseInheritedDefault={onUseInheritedDefault}
      onManagePersonas={vi.fn()}
      {...overrides}
    />,
  );
  return { onUpdate, onExpandedChange, onUseInheritedDefault };
}

describe("AgentRoleDefaultRow", () => {
  it("labels bridged legacy workspace-review defaults", () => {
    render(
      <AgentRoleDefaultRow
        entry={{
          ...entry,
          configured: null,
          source: "legacy_workspace_review",
        }}
        expanded={false}
        disabled={false}
        providers={["codex"]}
        modelsForProvider={() => []}
        personas={[]}
        onUpdate={vi.fn()}
        onExpandedChange={vi.fn()}
        onUseInheritedDefault={vi.fn().mockResolvedValue(true)}
        onManagePersonas={vi.fn()}
      />,
    );

    expect(screen.getByText("Inherited · Legacy Workspace Review"))
      .toBeInTheDocument();
  });

  it("renders a compact configured summary without mounting editor controls", () => {
    renderRow();

    expect(screen.getByText(entry.description)).toBeInTheDocument();
    expect(screen.getByText("Configured here")).toBeInTheDocument();
    expect(screen.getByText("Claude")).toBeInTheDocument();
    expect(screen.getByText("Sonnet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit Workspace Project" }))
      .toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("combobox", { name: "Workspace Project provider" }))
      .not.toBeInTheDocument();
  });

  it("opens diagnostics and the complete editor without parsing diagnostic text", () => {
    const { onExpandedChange } = renderRow({
      entry: {
        ...entry,
        diagnostics: ["An unstructured backend diagnostic"],
      },
    });

    expect(screen.getByRole("alert")).toHaveTextContent("An unstructured backend diagnostic");
    expect(screen.getByRole("button", { name: /^Runtime:/ }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Permissions" }))
      .toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("combobox", { name: "Workspace Project approval policy" }))
      .toBeInTheDocument();
    screen.getByRole("button", { name: "Edit Workspace Project" }).click();
    expect(onExpandedChange).not.toHaveBeenCalled();
  });

  it("requests disclosure without making the summary itself editable", async () => {
    const user = userEvent.setup();
    const { onExpandedChange } = renderRow();

    await user.click(screen.getByRole("button", { name: "Edit Workspace Project" }));

    expect(onExpandedChange).toHaveBeenCalledWith(true);
  });

  it("keeps inheritance confirmation pending through clear settlement", async () => {
    const user = userEvent.setup();
    let resolveClear: (() => void) | undefined;
    const onUseInheritedDefault = vi.fn(
      () => new Promise<void>((resolve) => { resolveClear = resolve; }),
    );
    renderRow({ onUseInheritedDefault });

    await user.click(screen.getByRole("button", {
      name: "Use inherited default for Workspace Project",
    }));
    expect(screen.getByText(/removes the UI override at this scope/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Use inherited default" }));

    expect(screen.getByRole("button", { name: "Using inherited default..." }))
      .toBeDisabled();
    expect(onUseInheritedDefault).toHaveBeenCalledOnce();

    resolveClear?.();
    await waitFor(() => {
      expect(screen.queryByText(/removes the UI override at this scope/i))
        .not.toBeInTheDocument();
    });
  });

  it("keeps a failed clear retryable and does not alter the configured summary", async () => {
    const user = userEvent.setup();
    const onUseInheritedDefault = vi.fn().mockRejectedValue(new Error("Clear failed"));
    renderRow({ onUseInheritedDefault });

    await user.click(screen.getByRole("button", {
      name: "Use inherited default for Workspace Project",
    }));
    await user.click(screen.getByRole("button", { name: "Use inherited default" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled());
    expect(screen.getByText("Configured here")).toBeInTheDocument();
  });
});
