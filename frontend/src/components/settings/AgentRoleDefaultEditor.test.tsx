import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ManualRoleCatalogEntry, ManualRoleDefault } from "@/api/manual-role-defaults.types";

import { AgentRoleDefaultEditor } from "./AgentRoleDefaultEditor";

if (!HTMLElement.prototype.hasPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "hasPointerCapture", {
    value: () => false,
    writable: true,
  });
}

if (!HTMLElement.prototype.setPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "setPointerCapture", {
    value: vi.fn(),
    writable: true,
  });
}

if (!HTMLElement.prototype.scrollIntoView) {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    value: vi.fn(),
    writable: true,
  });
}

const value: ManualRoleDefault = {
  provider: "claude",
  model: "sonnet",
  effort: "high",
  serviceTier: "provider_default",
  coordinationMode: "solo",
  personaId: null,
  approvalPolicy: null,
  sandboxMode: null,
  atlassianAccess: null,
};

const entry: ManualRoleCatalogEntry = {
  role: "workspace_edit",
  displayName: "Edit",
  description: "Implements requested changes in the project workspace.",
  family: "workspace",
  familyDisplayName: "Workspace",
  configured: null,
  effective: value,
  source: "provider_default",
  diagnostics: [],
  controls: {
    capabilities: [
      { value: "solo", enabled: true, disabledReason: null },
      { value: "rx_native_team", enabled: false, disabledReason: "Team is currently unavailable" },
    ],
    speeds: [
      { value: "provider_default", enabled: true, disabledReason: null },
      { value: "fast", enabled: false, disabledReason: "Fast is unavailable for this model" },
    ],
    persona: { enabled: false, disabledReason: "Personas are unavailable for this role" },
  },
};

function renderEditor(
  onUpdate = vi.fn(),
  forcePersonaAccessOpen = false,
  onManagePersonas = vi.fn(),
  atlassianAvailable = true,
) {
  render(
    <AgentRoleDefaultEditor
      entry={entry}
      disabled={false}
      atlassianAvailable={atlassianAvailable}
      providers={["claude", "codex"]}
      modelsForProvider={(provider) => provider === "codex"
        ? [{ id: "gpt-5.6", label: "GPT-5.6", menuLabel: "GPT-5.6", defaultEffort: "xhigh", supportedEfforts: ["xhigh"] }]
        : [{ id: "sonnet", label: "Sonnet", menuLabel: "Sonnet", defaultEffort: "high", supportedEfforts: ["high"] }]}
      personas={[]}
      forcePersonaAccessOpen={forcePersonaAccessOpen}
      onUpdate={onUpdate}
      onManagePersonas={onManagePersonas}
    />,
  );
  return onUpdate;
}

describe("AgentRoleDefaultEditor", () => {
  it("writes one complete value and preserves provider-model-effort coupling", async () => {
    const user = userEvent.setup();
    const onUpdate = renderEditor();

    await user.click(screen.getByTestId("agent-composer-runtime-pill"));
    await user.click(
      screen.getByTestId("agent-composer-runtime-provider-menu-trigger"),
    );
    await user.click(screen.getByTestId("agent-composer-runtime-provider-codex"));

    expect(onUpdate).toHaveBeenCalledWith({
      ...value,
      provider: "codex",
      model: "gpt-5.6",
      effort: "xhigh",
    });
  });

  it("keeps advanced controls secondary and places backend-disabled reasons with them", async () => {
    const user = userEvent.setup();
    renderEditor();

    await user.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Capabilities,/ }));
    expect(screen.getByText("Team is currently unavailable")).toBeInTheDocument();
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Speed,/ }));
    expect(screen.getByText("Fast is unavailable for this model")).toBeInTheDocument();
    const personaTrigger = screen.getByTestId(
      "agent-composer-runtime-persona-menu-trigger",
    );
    fireEvent.pointerMove(personaTrigger);
    expect(
      screen.getByText("Personas are unavailable for this role"),
    ).toBeInTheDocument();

    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Permissions" }));
    expect(screen.getByRole("combobox", { name: "Edit approval policy" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Edit sandbox mode" })).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: "Edit Atlassian access" }),
    ).toBeInTheDocument();
  });

  it("keeps persona management reachable when the backend disables selection", async () => {
    const user = userEvent.setup();
    const onManagePersonas = vi.fn();
    renderEditor(vi.fn(), false, onManagePersonas);

    await user.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.pointerMove(
      screen.getByTestId("agent-composer-runtime-persona-menu-trigger"),
    );
    await user.click(screen.getByRole("button", { name: "Manage personas" }));

    expect(onManagePersonas).toHaveBeenCalledTimes(1);
  });

  it("keeps diagnostic-forced persona and access controls visibly open", async () => {
    const user = userEvent.setup();
    renderEditor(vi.fn(), true);

    const disclosure = screen.getByRole("button", { name: "Permissions" });
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    expect(disclosure).toHaveAttribute("aria-disabled", "true");

    await user.click(disclosure);

    expect(screen.getByRole("combobox", { name: "Edit approval policy" })).toBeInTheDocument();
  });

  it("commits an Atlassian tier as one complete value", async () => {
    const user = userEvent.setup();
    const onUpdate = renderEditor();

    await user.click(screen.getByRole("button", { name: "Permissions" }));
    await user.click(
      screen.getByRole("combobox", { name: "Edit Atlassian access" }),
    );
    await user.click(screen.getByRole("option", { name: "Read + write" }));

    expect(onUpdate).toHaveBeenCalledWith({
      ...value,
      atlassianAccess: "read_write",
    });
  });

  it("returns the Atlassian tier to the role default via the sentinel option", async () => {
    const user = userEvent.setup();
    const onUpdate = vi.fn();
    render(
      <AgentRoleDefaultEditor
        entry={{
          ...entry,
          configured: { ...value, atlassianAccess: "none" },
        }}
        disabled={false}
        providers={["claude"]}
        modelsForProvider={() => [
          {
            id: "sonnet",
            label: "Sonnet",
            menuLabel: "Sonnet",
            defaultEffort: "high",
            supportedEfforts: ["high"],
          },
        ]}
        personas={[]}
        forcePersonaAccessOpen={false}
        onUpdate={onUpdate}
        onManagePersonas={vi.fn()}
      />,
    );

    // A configured Atlassian tier opens the Permissions section on its own.
    await user.click(
      screen.getByRole("combobox", { name: "Edit Atlassian access" }),
    );
    await user.click(screen.getByRole("option", { name: "Role default" }));

    expect(onUpdate).toHaveBeenCalledWith({
      ...value,
      atlassianAccess: null,
    });
  });

  it("disables the Atlassian tier with a hint when the integration is unavailable", async () => {
    const user = userEvent.setup();
    renderEditor(vi.fn(), false, vi.fn(), false);

    await user.click(screen.getByRole("button", { name: "Permissions" }));

    const control = screen.getByRole("combobox", { name: "Edit Atlassian access" });
    expect(control).toBeDisabled();
    expect(
      screen.getByText(/Connect Atlassian in Settings > Integrations/),
    ).toBeInTheDocument();
    // Approval and sandbox stay usable; only the Atlassian tier is gated.
    expect(
      screen.getByRole("combobox", { name: "Edit approval policy" }),
    ).not.toBeDisabled();
  });
});
