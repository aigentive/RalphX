import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LeftNavRail } from "./LeftNavRail";
import type { FeatureFlags } from "@/types/feature-flags";

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn((selector: (s: { activeProjectId: string | null }) => unknown) =>
    selector({ activeProjectId: "project-1" }),
  ),
}));

let mockFeatureFlags: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  automationsPage: true,
  atlassianOauth: false,
  ticketingDashboard: true,
};

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: vi.fn(() => ({ data: mockFeatureFlags })),
}));

let mockTicketingProviders: Array<{
  provider: string;
  enabled: boolean;
  connectionStatus: "connected" | "disconnected" | "permission_limited" | "error";
}> = [];
let mockGranolaConnected = true;

vi.mock("@/hooks/useTicketing", () => ({
  useTicketingProviders: vi.fn(() => ({ data: mockTicketingProviders })),
}));

vi.mock("@/hooks/useGranolaIntegration", () => ({
  useGranolaIntegration: vi.fn(() => ({ connected: mockGranolaConnected })),
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
      automationsPage: true,
      atlassianOauth: false,
      ticketingDashboard: true,
    };
    // Default: one connected/enabled ticketing provider so the Ticketing entry is
    // visible for the grouping/warm-up assertions below.
    mockTicketingProviders = [
      { provider: "linear", enabled: true, connectionStatus: "connected" },
    ];
    mockGranolaConnected = true;
  });

  it("separates dashboard access from primary mini-sidebar views", () => {
    const onViewChange = vi.fn();

    render(<LeftNavRail currentView="agents" onViewChange={onViewChange} />);

    const separator = screen.getByTestId("nav-dashboard-separator");
    const automationsButton = screen.getByTestId("nav-automations");
    const ticketingButton = screen.getByTestId("nav-ticketing");
    const githubButton = screen.getByTestId("nav-github");
    const granolaButton = screen.getByTestId("nav-granola");
    expect(automationsButton).toBeInTheDocument();
    expect(separator).toBeInTheDocument();
    expect(ticketingButton).toBeInTheDocument();
    expect(githubButton).toBeInTheDocument();
    expect(granolaButton).toBeInTheDocument();
    expect(automationsButton.compareDocumentPosition(separator)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(separator.compareDocumentPosition(ticketingButton)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(ticketingButton.compareDocumentPosition(githubButton)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(githubButton.compareDocumentPosition(granolaButton)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );

    fireEvent.click(ticketingButton);
    expect(onViewChange).toHaveBeenCalledWith("ticketing");

    fireEvent.click(githubButton);
    expect(onViewChange).toHaveBeenCalledWith("github");

    fireEvent.click(granolaButton);
    expect(onViewChange).toHaveBeenCalledWith("granola");
  });

  it("keeps Insights visible when task-pipeline stats are sparse", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.getByTestId("nav-insights")).toBeInTheDocument();
  });

  it("hides the Ticketing entry but keeps GitHub and Granola when no ticketing provider is enabled", () => {
    mockTicketingProviders = [
      { provider: "linear", enabled: false, connectionStatus: "disconnected" },
      { provider: "jira", enabled: false, connectionStatus: "disconnected" },
      { provider: "clickup", enabled: false, connectionStatus: "disconnected" },
    ];

    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.queryByTestId("nav-ticketing")).not.toBeInTheDocument();
    expect(screen.getByTestId("nav-github")).toBeInTheDocument();
    expect(screen.getByTestId("nav-granola")).toBeInTheDocument();
    expect(screen.getByTestId("nav-dashboard-separator")).toBeInTheDocument();
  });

  it("shows the Ticketing entry when the old dashboard feature flag is off but a provider is valid", () => {
    mockFeatureFlags = { ...mockFeatureFlags, ticketingDashboard: false };
    mockTicketingProviders = [
      { provider: "jira", enabled: false, connectionStatus: "disconnected" },
      { provider: "linear", enabled: false, connectionStatus: "disconnected" },
      { provider: "clickup", enabled: true, connectionStatus: "connected" },
    ];

    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.getByTestId("nav-dashboard-separator")).toBeInTheDocument();
    expect(screen.getByTestId("nav-ticketing")).toBeInTheDocument();
  });

  it("shows the Ticketing entry when a provider is valid", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.getByTestId("nav-dashboard-separator")).toBeInTheDocument();
    expect(screen.getByTestId("nav-ticketing")).toBeInTheDocument();
  });

  it("hides the Ticketing entry but keeps GitHub and Granola when providers are enabled but not connected", () => {
    mockTicketingProviders = [
      { provider: "linear", enabled: true, connectionStatus: "error" },
      { provider: "jira", enabled: true, connectionStatus: "permission_limited" },
    ];

    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.queryByTestId("nav-ticketing")).not.toBeInTheDocument();
    expect(screen.getByTestId("nav-github")).toBeInTheDocument();
    expect(screen.getByTestId("nav-granola")).toBeInTheDocument();
    expect(screen.getByTestId("nav-dashboard-separator")).toBeInTheDocument();
  });

  it("hides the Granola entry until the API token is configured and valid", () => {
    mockGranolaConnected = false;

    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.getByTestId("nav-github")).toBeInTheDocument();
    expect(screen.queryByTestId("nav-granola")).not.toBeInTheDocument();
  });

  it("hides the Automations entry when the feature flag is off", () => {
    mockFeatureFlags = { ...mockFeatureFlags, automationsPage: false };

    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(screen.queryByTestId("nav-automations")).not.toBeInTheDocument();
  });

  it("filters dashboard items out of the primary nav section", () => {
    render(
      <LeftNavRail
        currentView="agents"
        onViewChange={vi.fn()}
      />,
    );

    // The Dashboard group wraps ticketing/GitHub/Granola items; the primary <nav> should not.
    const dashboardGroup = screen.getByRole("group", { name: "Dashboard" });
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-ticketing"));
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-github"));
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-granola"));
    // Primary views like Agents/Automations are NOT inside the Dashboard group.
    expect(dashboardGroup).not.toContainElement(screen.getByTestId("nav-agents"));
    expect(dashboardGroup).not.toContainElement(screen.getByTestId("nav-automations"));
  });

  it("keeps only Agents task workflows in the primary rail with contiguous shortcuts", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    const agents = screen.getByTestId("nav-agents");
    const automations = screen.getByTestId("nav-automations");
    const insights = screen.getByTestId("nav-insights");

    expect(agents).toHaveTextContent("Agents");
    expect(screen.getByText("⌘1")).toBeInTheDocument();
    expect(automations).toHaveTextContent("Automations");
    expect(screen.getByText("⌘2")).toBeInTheDocument();
    expect(insights).toHaveTextContent("Insights");
    expect(screen.getByText("⌘3")).toBeInTheDocument();
    expect(agents.compareDocumentPosition(automations)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(automations.compareDocumentPosition(insights)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(screen.queryByTestId("nav-ideation")).not.toBeInTheDocument();
    expect(screen.queryByTestId("nav-graph")).not.toBeInTheDocument();
    expect(screen.queryByTestId("nav-kanban")).not.toBeInTheDocument();
  });

  it("renders the dashboard items in a group labeled Dashboard", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    const dashboardGroup = screen.getByRole("group", { name: "Dashboard" });
    expect(dashboardGroup).toBeInTheDocument();
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-ticketing"));
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-github"));
    expect(dashboardGroup).toContainElement(screen.getByTestId("nav-granola"));
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

    const insightsButton = screen.getByTestId("nav-insights");
    fireEvent.pointerEnter(insightsButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("insights");

    onViewWarmUp.mockClear();
    fireEvent.focus(insightsButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("insights");

    onViewWarmUp.mockClear();
    const automationsButton = screen.getByTestId("nav-automations");
    fireEvent.pointerEnter(automationsButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("automations");
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

    onViewWarmUp.mockClear();
    const githubButton = screen.getByTestId("nav-github");
    fireEvent.pointerEnter(githubButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("github");

    onViewWarmUp.mockClear();
    fireEvent.focus(githubButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("github");

    onViewWarmUp.mockClear();
    const granolaButton = screen.getByTestId("nav-granola");
    fireEvent.pointerEnter(granolaButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("granola");

    onViewWarmUp.mockClear();
    fireEvent.focus(granolaButton);
    expect(onViewWarmUp).toHaveBeenCalledWith("granola");
  });

  it("does not throw when onViewWarmUp is not provided", () => {
    render(<LeftNavRail currentView="agents" onViewChange={vi.fn()} />);

    expect(() =>
      fireEvent.pointerEnter(screen.getByTestId("nav-ticketing")),
    ).not.toThrow();
  });
});
