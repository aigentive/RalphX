import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { TicketingFilterState } from "@/stores/ticketingStore";

import { TicketFilterBar } from "./TicketFilterBar";
import { UNASSIGNED_ASSIGNEE } from "./ticketing-read-state";

const baseFilters: TicketingFilterState = { text: "", assignee: null, stateIds: [], labels: [] };

function renderBar(overrides: Partial<Parameters<typeof TicketFilterBar>[0]> = {}) {
  const props = {
    containers: [],
    columns: [],
    assigneeOptions: ["Ada", "Grace"],
    containerLabel: "Project",
    allContainersLabel: "All projects",
    activeContainerId: null,
    filters: baseFilters,
    viewMode: "list" as const,
    isRefreshing: false,
    onContainerChange: vi.fn(),
    onFiltersChange: vi.fn(),
    onResetFilters: vi.fn(),
    onViewModeChange: vi.fn(),
    onRefresh: vi.fn(),
    ...overrides,
  };
  render(
    <TooltipProvider>
      <TicketFilterBar {...props} />
    </TooltipProvider>,
  );
  return props;
}

describe("TicketFilterBar", () => {
  it("hides the Reset button when no filters are active", () => {
    renderBar();
    expect(screen.queryByRole("button", { name: /reset/i })).not.toBeInTheDocument();
  });

  it("shows the Reset button once a filter is active", () => {
    renderBar({ filters: { ...baseFilters, text: "race" } });
    expect(screen.getByRole("button", { name: /reset/i })).toBeInTheDocument();
  });

  it("renders Everyone, Unassigned, and each assignee option", () => {
    renderBar();
    const select = screen.getByRole("combobox", { name: /assignee/i });
    const options = within(select).getAllByRole("option").map((option) => option.textContent);
    expect(options).toEqual(["Everyone", "Unassigned", "Ada", "Grace"]);
  });

  it("emits the selected assignee, the unassigned sentinel, and null for everyone", () => {
    const props = renderBar({ filters: { ...baseFilters, assignee: "Ada" } });
    const select = screen.getByRole("combobox", { name: /assignee/i });

    fireEvent.change(select, { target: { value: "Grace" } });
    expect(props.onFiltersChange).toHaveBeenCalledWith({ assignee: "Grace" });

    fireEvent.change(select, { target: { value: UNASSIGNED_ASSIGNEE } });
    expect(props.onFiltersChange).toHaveBeenCalledWith({ assignee: UNASSIGNED_ASSIGNEE });

    fireEvent.change(select, { target: { value: "" } });
    expect(props.onFiltersChange).toHaveBeenCalledWith({ assignee: null });
  });
});
