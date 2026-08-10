import { LOCAL_ENVIRONMENT_ID } from "@/stores/environmentStore";
import { resetTransportEnvironmentId } from "@/lib/remote/active-environment";
import { resetQueryClient } from "@/lib/queryClient";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useEnvironmentStore } from "@/stores/environmentStore";
import type { Persona } from "@/types/persona";

import { PersonaRow } from "./PersonaManagementRows";

// Gate tests park the store on a remote environment; without this the next file in
// the same worker inherits it and resolves a different keyed QueryClient. That is
// what broke EnvironmentScopedProviders under CI sharding.
afterEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
});

const persona: Persona = {
  id: "persona-1",
  slug: "reviewer",
  name: "Reviewer",
  description: "Reviews carefully",
  content: "Review carefully",
  status: "active",
  version: 1,
  projectId: null,
  contentHash: "hash-1",
  sourceSessionId: null,
  sourcePersonaId: null,
  sourceContentHash: null,
  artifactId: null,
  createdAt: "2026-08-01T00:00:00Z",
  updatedAt: "2026-08-01T00:00:00Z",
};

function setRemote(scopes: readonly string[]): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: "remote",
    environments: [
      { id: "local", name: "This Mac", kind: "local" },
      { id: "remote", name: "Studio Mac", kind: "remote" },
    ],
    effectiveScopes: { remote: scopes },
    connectionPresentations: {
      remote: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
  });
}

function renderRow(onRemove: (selected: Persona) => void): void {
  render(
    <TooltipProvider delayDuration={0}>
      <PersonaRow
        persona={persona}
        projectNames={{}}
        onEdit={vi.fn()}
        onActivate={vi.fn()}
        onRemove={onRemove}
        onRefine={vi.fn()}
        onRestore={vi.fn()}
      />
    </TooltipProvider>,
  );
}

describe("PersonaRow agent gate", () => {
  beforeEach(() => setRemote(["ui:read", "ui:operate"]));

  it("soft-disables archive, exposes its reason on focus, and suppresses dispatch", async () => {
    const onRemove = vi.fn();
    renderRow(onRemove);
    const button = screen.getByRole("button", { name: "Archive Reviewer" });

    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("data-disabled-explained", "true");
    expect(button).not.toBeDisabled();
    fireEvent.click(button);
    expect(onRemove).not.toHaveBeenCalled();
    button.focus();
    expect(
      (
        await screen.findAllByText(
          "Agent control is off for this device — enable it on the host.",
        )
      ).length,
    ).toBeGreaterThan(0);
  });

  it("keeps archive live and dispatches when agent control is granted", () => {
    setRemote(["ui:read", "ui:operate", "ui:agent"]);
    const onRemove = vi.fn();
    renderRow(onRemove);

    fireEvent.click(screen.getByRole("button", { name: "Archive Reviewer" }));
    expect(onRemove).toHaveBeenCalledWith(persona);
  });
});
