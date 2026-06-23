import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LeftNavRail } from "./LeftNavRail";
import type { FeatureFlags } from "@/types/feature-flags";

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn((selector: (s: { activeProjectId: string | null }) => unknown) =>
    selector({ activeProjectId: "project-1" }),
  ),
}));

vi.mock("@/hooks/useProjectStats", () => ({
  useProjectStats: vi.fn(() => ({
    data: { taskCount: 0 },
    isLoading: false,
    isError: false,
  })),
}));

let mockFeatureFlags: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
  ticketingDashboard: false,
};

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: vi.fn(() => ({ data: mockFeatureFlags })),
}));

vi.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
}));

describe("LeftNavRail", () => {
  beforeEach(() => {
    mockFeatureFlags = {
      activityPage: true,
      extensibilityPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
    };
  });

  it("separates dashboard access from primary mini-sidebar views", () => {
    mockFeatureFlags = {
      ...mockFeatureFlags,
      ticketingDashboard: true,
    };
    const onViewChange = vi.fn();

    render(<LeftNavRail currentView="agents" onViewChange={onViewChange} />);

    const separator = screen.getByTestId("nav-dashboard-separator");
    const ticketingButton = screen.getByTestId("nav-ticketing");
    expect(separator).toBeInTheDocument();
    expect(ticketingButton).toBeInTheDocument();
    expect(separator.compareDocumentPosition(ticketingButton)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );

    fireEvent.click(ticketingButton);

    expect(onViewChange).toHaveBeenCalledWith("ticketing");
  });

  it("keeps dashboard access discoverable when the dashboard feature is disabled", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.getByTestId("nav-dashboard-separator")).toBeInTheDocument();
    expect(screen.getByTestId("nav-ticketing")).toBeInTheDocument();
  });

  it("filters the ticketing item out of the primary nav section", () => {
    render(
      <LeftNavRail
        currentView="agents"
        onViewChange={vi.fn()}
      />,
    );

    // The Dashboard group wraps the ticketing item; the primary <nav> should not.
    const dashboardGroup = screen.getByRole("group", { name: "Dashboard" });
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-ticketing"));
    // Primary views like Agents/Kanban are NOT inside the Dashboard group.
    expect(dashboardGroup).not.toContainElement(screen.getByTestId("nav-agents"));
    expect(dashboardGroup).not.toContainElement(screen.getByTestId("nav-kanban"));
  });

  it("renders the dashboard items in a group labeled Dashboard", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    const dashboardGroup = screen.getByRole("group", { name: "Dashboard" });
    expect(dashboardGroup).toBeInTheDocument();
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-ticketing"));
  });

  it("warms up a primary view on pointer enter and focus", () => {
    const onViewWarmUp = vi.fn();
    render(
      <LeftNavRail
        currentView="agents"
        onViewChange={vi.fn()}
        onViewWarmUp={onViewWarmUp}
      />,
    );

    const kanbanButton = screen.getByTestId("nav-kanban");
    fireEvent.pointerEnter(kanbanButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("kanban");

    onViewWarmUp.mockClear();
    fireEvent.focus(kanbanButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("kanban");
  });

  it("warms up the ticketing dashboard item on pointer enter and focus", () => {
    const onViewWarmUp = vi.fn();
    render(
      <LeftNavRail
        currentView="agents"
        onViewChange={vi.fn()}
        onViewWarmUp={onViewWarmUp}
      />,
    );

    const ticketingButton = screen.getByTestId("nav-ticketing");
    fireEvent.pointerEnter(ticketingButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("ticketing");

    onViewWarmUp.mockClear();
    fireEvent.focus(ticketingButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("ticketing");
  });

  it("does not throw when onViewWarmUp is not provided", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(() =>
      fireEvent.pointerEnter(screen.getByTestId("nav-ticketing")),
    ).not.toThrow();
  });
});
