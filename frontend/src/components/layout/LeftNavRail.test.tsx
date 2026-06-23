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
  ticketingDashboard: true,
};

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: vi.fn(() => ({ data: mockFeatureFlags })),
}));

let mockTicketingProviders: Array<{ provider: string; enabled: boolean }> = [];

vi.mock("@/hooks/useTicketing", () => ({
  useTicketingProviders: vi.fn(() => ({ data: mockTicketingProviders })),
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
      ticketingDashboard: true,
    };
    // Default: one connected/enabled ticketing provider so the Ticketing entry is
    // visible for the grouping/warm-up assertions below.
    mockTicketingProviders = [{ provider: "linear", enabled: true }];
  });

  it("separates dashboard access from primary mini-sidebar views", () => {
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

  it("hides the Ticketing entry when no Linear/Jira provider is enabled", () => {
    mockTicketingProviders = [
      { provider: "linear", enabled: false },
      { provider: "jira", enabled: false },
    ];

    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.queryByTestId("nav-ticketing")).not.toBeInTheDocument();
    expect(screen.queryByTestId("nav-dashboard-separator")).not.toBeInTheDocument();
  });

  it("hides the Ticketing entry when the dashboard feature flag is off", () => {
    mockFeatureFlags = { ...mockFeatureFlags, ticketingDashboard: false };

    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.queryByTestId("nav-ticketing")).not.toBeInTheDocument();
    expect(screen.queryByTestId("nav-dashboard-separator")).not.toBeInTheDocument();
  });

  it("shows the Ticketing entry when the flag is on and a provider is enabled", () => {
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
