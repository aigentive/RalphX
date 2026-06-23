import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { TicketingFilterState } from "@/stores/ticketingStore";

import { TicketFilterBar } from "./TicketFilterBar";
import { UNASSIGNED_ASSIGNEE } from "./ticketing-read-state";

const baseFilters: TicketingFilterState = {
  text: "",
  assignee: null,
  stateIds: [],
  labels: [],
  sprint: null,
};

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
    fireEvent.click(screen.getByRole("combobox", { name: /assignee/i }));
    const options = within(screen.getByRole("listbox", { name: /assignee/i }))
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(options).toEqual(["Everyone", "Unassigned", "Ada", "Grace"]);
  });

  it("emits the selected assignee, the unassigned sentinel, and null for everyone", () => {
    const props = renderBar({ filters: { ...baseFilters, assignee: "Ada" } });
    const select = screen.getByRole("combobox", { name: /assignee/i });

    fireEvent.click(select);
    fireEvent.click(screen.getByRole("option", { name: "Grace" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ assignee: "Grace" });

    fireEvent.click(select);
    fireEvent.click(screen.getByRole("option", { name: "Unassigned" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ assignee: UNASSIGNED_ASSIGNEE });

    fireEvent.click(select);
    fireEvent.click(screen.getByRole("option", { name: "Everyone" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ assignee: null });
  });

  it("clears selected filter values from the select trigger", () => {
    const props = renderBar({
      containers: [{ provider: "jira", id: "project-1", name: "Reef", kind: "project" }],
      columns: [{ id: "todo", name: "To Do", category: "todo", order: 0 }],
      activeContainerId: "project-1",
      filters: { ...baseFilters, assignee: "Ada", stateIds: ["todo"], sprint: "Sprint 42" },
      sprintOptions: ["Sprint 41", "Sprint 42"],
    });

    fireEvent.click(screen.getByRole("button", { name: "Clear project filter" }));
    expect(props.onContainerChange).toHaveBeenCalledWith(null);

    fireEvent.click(screen.getByRole("button", { name: "Clear status filter" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ stateIds: [] });

    fireEvent.click(screen.getByRole("button", { name: "Clear assignee filter" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ assignee: null });

    fireEvent.click(screen.getByRole("button", { name: "Clear sprint filter" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ sprint: null });
  });

  it("filters select options through search", () => {
    renderBar();

    fireEvent.click(screen.getByRole("combobox", { name: /assignee/i }));
    fireEvent.change(screen.getByLabelText("Search assignee"), {
      target: { value: "gra" },
    });

    const listbox = screen.getByRole("listbox", { name: /assignee/i });
    expect(within(listbox).getByRole("option", { name: "Grace" })).toBeInTheDocument();
    expect(within(listbox).queryByRole("option", { name: "Ada" })).not.toBeInTheDocument();
  });

  it("reveals more select options when the option list scrolls near the bottom", () => {
    const assigneeOptions = Array.from(
      { length: 25 },
      (_, index) => `User ${String(index + 1).padStart(2, "0")}`,
    );
    renderBar({ assigneeOptions });

    fireEvent.click(screen.getByRole("combobox", { name: /assignee/i }));
    const listbox = screen.getByRole("listbox", { name: /assignee/i });
    expect(within(listbox).queryByRole("option", { name: "User 25" })).not.toBeInTheDocument();

    const scrollContainer = listbox.parentElement as HTMLElement;
    Object.defineProperty(scrollContainer, "scrollHeight", { configurable: true, value: 240 });
    Object.defineProperty(scrollContainer, "scrollTop", { configurable: true, value: 210 });
    Object.defineProperty(scrollContainer, "clientHeight", { configurable: true, value: 40 });
    fireEvent.scroll(scrollContainer);

    expect(within(listbox).getByRole("option", { name: "User 25" })).toBeInTheDocument();
  });

  it("renders an optional Sprint filter and emits the selected sprint", () => {
    const props = renderBar({
      filters: { ...baseFilters, sprint: "Sprint 42" },
      sprintOptions: ["Sprint 41", "Sprint 42"],
    });
    const select = screen.getByRole("combobox", { name: "Sprint" });

    fireEvent.click(select);
    const listbox = screen.getByRole("listbox", { name: "Sprint" });
    expect(within(listbox).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "All sprints",
      "Sprint 41",
      "Sprint 42",
    ]);

    fireEvent.click(within(listbox).getByRole("option", { name: "Sprint 41" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ sprint: "Sprint 41" });

    fireEvent.click(screen.getByRole("combobox", { name: "Sprint" }));
    fireEvent.click(screen.getByRole("option", { name: "All sprints" }));
    expect(props.onFiltersChange).toHaveBeenCalledWith({ sprint: null });
  });

  });

  it("gives every filter select an accessible name and the unified treatment", () => {
    renderBar();

    const container = screen.getByRole("combobox", { name: "Project" });
    const status = screen.getByRole("combobox", { name: "Status" });
    const assignee = screen.getByRole("combobox", { name: /assignee/i });

    for (const select of [container, status, assignee]) {
      expect(select.className).toContain("appearance-none");
      expect(select.className).toContain(
        "focus-visible:[outline:2px_solid_var(--border-focus)]",
      );
      expect((select as HTMLElement).style.backgroundColor).toBe("var(--bg-elevated)");
    }
  });
});
