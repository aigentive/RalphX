import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TicketingProvider, TicketingProviderSummary } from "@/api/ticketing";

import { ProviderSwitcher } from "./ProviderSwitcher";

function makeProvider(
  provider: TicketingProvider,
  label: string,
): TicketingProviderSummary {
  return {
    provider,
    label,
    enabled: true,
    connectionStatus: "connected",
    capabilities: {
      kanbanWrite: false,
      statusWrite: false,
      assignmentWrite: false,
      commentWrite: false,
      freshness: "manual",
    },
  };
}

describe("ProviderSwitcher", () => {
  it("renders nothing when there are no providers", () => {
    const { container } = render(
      <ProviderSwitcher providers={[]} activeProvider={null} onProviderChange={vi.fn()} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders a static badge (no tablist) for a single provider", () => {
    render(
      <ProviderSwitcher
        providers={[makeProvider("jira", "Jira")]}
        activeProvider="jira"
        onProviderChange={vi.fn()}
      />,
    );
    expect(screen.getByText("Jira")).toBeInTheDocument();
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  });

  it("renders a tablist with one tab per provider when there are multiple", () => {
    render(
      <ProviderSwitcher
        providers={[makeProvider("jira", "Jira"), makeProvider("linear", "Linear")]}
        activeProvider="linear"
        onProviderChange={vi.fn()}
      />,
    );
    expect(screen.getByRole("tablist", { name: "Ticketing provider" })).toBeInTheDocument();
    const tabs = screen.getAllByRole("tab");
    expect(tabs).toHaveLength(2);
  });

  it("marks the active provider tab as selected", () => {
    render(
      <ProviderSwitcher
        providers={[makeProvider("jira", "Jira"), makeProvider("linear", "Linear")]}
        activeProvider="linear"
        onProviderChange={vi.fn()}
      />,
    );
    expect(screen.getByRole("tab", { name: "Linear" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Jira" })).toHaveAttribute("aria-selected", "false");
  });

  it("invokes onProviderChange with the clicked provider", () => {
    const onProviderChange = vi.fn();
    render(
      <ProviderSwitcher
        providers={[makeProvider("jira", "Jira"), makeProvider("linear", "Linear")]}
        activeProvider="jira"
        onProviderChange={onProviderChange}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Linear" }));
    expect(onProviderChange).toHaveBeenCalledTimes(1);
    expect(onProviderChange).toHaveBeenCalledWith("linear");
  });
});
