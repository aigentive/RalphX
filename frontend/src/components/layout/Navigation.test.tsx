/**
 * Navigation component tests — team pill visibility + teammate count + feature flags
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { Navigation } from "./Navigation";
import type { FeatureFlags } from "@/types/feature-flags";

// Dynamic mock state
let mockState = {
  activeTeams: {} as Record<string, { teammates: Record<string, unknown> }>,
};

vi.mock("@/stores/teamStore", () => ({
  useTeamStore: (selector: (state: typeof mockState) => unknown) => selector(mockState),
  selectHasAnyActiveTeam: (state: typeof mockState) =>
    Object.keys(state.activeTeams).length > 0,
  selectTotalTeammateCount: (state: typeof mockState) =>
    Object.values(state.activeTeams).reduce(
      (sum: number, team) => sum + Object.keys(team.teammates).length,
      0,
    ),
}));

// Mock useProjectStore — Navigation reads activeProjectId from it
vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn((selector: (s: { activeProjectId: string | null }) => unknown) =>
    selector({ activeProjectId: "proj-1" })
  ),
}));

// Mock useProjectStats — avoids needing QueryClientProvider in Navigation tests
let mockTaskCount = 0;
vi.mock("@/hooks/useProjectStats", () => ({
  useProjectStats: vi.fn(() => ({
    data: { taskCount: mockTaskCount },
    isLoading: false,
    isError: false,
  })),
}));

// Feature flags mock — default all enabled
let mockFeatureFlags: FeatureFlags = { activityPage: true, extensibilityPage: true };

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: vi.fn(() => ({ data: mockFeatureFlags })),
}));

// Mock tooltip components to avoid portal issues
vi.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
}));

describe("Navigation", () => {
  const defaultProps = {
    currentView: "agents" as const,
    onViewChange: vi.fn(),
  };

  beforeEach(() => {
    mockState = { activeTeams: {} };
    mockFeatureFlags = { activityPage: true, extensibilityPage: true };
    mockTaskCount = 0;
  });

  it("renders all nav items", () => {
    render(<Navigation {...defaultProps} />);

    expect(screen.getByTestId("nav-agents")).toBeInTheDocument();
    expect(screen.getByTestId("nav-ideation")).toBeInTheDocument();
    expect(screen.getByTestId("nav-kanban")).toBeInTheDocument();
    expect(screen.getByTestId("nav-graph")).toBeInTheDocument();
    expect(screen.getByTestId("nav-activity")).toBeInTheDocument();
  });

  it("renders Agents first in the main navbar", () => {
    render(<Navigation {...defaultProps} />);

    const nav = screen.getByRole("navigation");
    const navItemIds = Array.from(nav.querySelectorAll("[data-testid]")).map((element) =>
      element.getAttribute("data-testid")
    );

    expect(navItemIds.slice(0, 4)).toEqual([
      "nav-agents",
      "nav-ideation",
      "nav-graph",
      "nav-kanban",
    ]);
  });

  it("orders the shortcut-backed main views as Agents, Ideation, Graph, Kanban, Insights", () => {
    mockTaskCount = 10;

    render(<Navigation {...defaultProps} />);

    const nav = screen.getByRole("navigation");
    const navItemIds = Array.from(nav.querySelectorAll("[data-testid]")).map((element) =>
      element.getAttribute("data-testid")
    );

    expect(navItemIds.slice(0, 5)).toEqual([
      "nav-agents",
      "nav-ideation",
      "nav-graph",
      "nav-kanban",
      "nav-insights",
    ]);
    expect(screen.getByText("⌘1")).toBeInTheDocument();
    expect(screen.getByText("⌘2")).toBeInTheDocument();
    expect(screen.getByText("⌘3")).toBeInTheDocument();
    expect(screen.getByText("⌘4")).toBeInTheDocument();
    expect(screen.getByText("⌘5")).toBeInTheDocument();
  });

  it("shows team pill when hasActiveTeam is true", () => {
    mockState = {
      activeTeams: {
        "ctx-1": { teammates: { "t-1": {}, "t-2": {}, "t-3": {}, "t-4": {} } },
      },
    };

    render(<Navigation {...defaultProps} />);

    expect(screen.getByText("4")).toBeInTheDocument();
  });

  it("displays correct teammate count in pill", () => {
    mockState = {
      activeTeams: {
        "ctx-1": { teammates: { "t-1": {}, "t-2": {}, "t-3": {} } },
        "ctx-2": { teammates: { "t-4": {}, "t-5": {}, "t-6": {}, "t-7": {} } },
      },
    };

    render(<Navigation {...defaultProps} />);

    expect(screen.getByText("7")).toBeInTheDocument();
  });

  it("hides team pill when no active teams", () => {
    mockState = { activeTeams: {} };

    render(<Navigation {...defaultProps} />);

    // No teammate count element should exist
    const nav = screen.getByRole("navigation");
    expect(nav.querySelector(".rounded-full")).toBeNull();
  });
});

describe("Navigation — feature flag filtering", () => {
  const defaultProps = {
    currentView: "agents" as const,
    onViewChange: vi.fn(),
  };

  beforeEach(() => {
    mockState = { activeTeams: {} };
    mockFeatureFlags = { activityPage: true, extensibilityPage: true };
    mockTaskCount = 0;
  });

  it("renders activity and extensibility nav items when flags are enabled", () => {
    mockFeatureFlags = { activityPage: true, extensibilityPage: true };

    render(<Navigation {...defaultProps} />);

    expect(screen.getByTestId("nav-activity")).toBeInTheDocument();
    expect(screen.getByTestId("nav-extensibility")).toBeInTheDocument();
  });

  it("hides activity nav item when activityPage flag is false", () => {
    mockFeatureFlags = { activityPage: false, extensibilityPage: true };

    render(<Navigation {...defaultProps} />);

    expect(screen.queryByTestId("nav-activity")).toBeNull();
    expect(screen.getByTestId("nav-extensibility")).toBeInTheDocument();
  });

  it("hides extensibility nav item when extensibilityPage flag is false", () => {
    mockFeatureFlags = { activityPage: true, extensibilityPage: false };

    render(<Navigation {...defaultProps} />);

    expect(screen.getByTestId("nav-activity")).toBeInTheDocument();
    expect(screen.queryByTestId("nav-extensibility")).toBeNull();
  });

  it("hides both activity and extensibility when both flags are false", () => {
    mockFeatureFlags = { activityPage: false, extensibilityPage: false };

    render(<Navigation {...defaultProps} />);

    expect(screen.queryByTestId("nav-activity")).toBeNull();
    expect(screen.queryByTestId("nav-extensibility")).toBeNull();
  });

  it("always renders core nav items regardless of flags", () => {
    mockFeatureFlags = { activityPage: false, extensibilityPage: false };

    render(<Navigation {...defaultProps} />);

    expect(screen.getByTestId("nav-agents")).toBeInTheDocument();
    expect(screen.getByTestId("nav-ideation")).toBeInTheDocument();
    expect(screen.getByTestId("nav-graph")).toBeInTheDocument();
    expect(screen.getByTestId("nav-kanban")).toBeInTheDocument();
    expect(screen.getByTestId("nav-settings")).toBeInTheDocument();
  });
});

describe("Navigation — hideViews (welcome mode)", () => {
  const defaultProps = {
    currentView: "agents" as const,
    onViewChange: vi.fn(),
    onOpenSettings: vi.fn(),
  };

  beforeEach(() => {
    mockState = { activeTeams: {} };
    mockFeatureFlags = { activityPage: true, extensibilityPage: true };
    mockTaskCount = 10;
  });

  it("hides every view nav item when hideViews is true", () => {
    render(<Navigation {...defaultProps} hideViews />);

    expect(screen.queryByTestId("nav-agents")).toBeNull();
    expect(screen.queryByTestId("nav-ideation")).toBeNull();
    expect(screen.queryByTestId("nav-graph")).toBeNull();
    expect(screen.queryByTestId("nav-kanban")).toBeNull();
    expect(screen.queryByTestId("nav-insights")).toBeNull();
    expect(screen.queryByTestId("nav-activity")).toBeNull();
    expect(screen.queryByTestId("nav-extensibility")).toBeNull();
  });

  it("still renders the Settings button when hideViews is true", () => {
    render(<Navigation {...defaultProps} hideViews />);

    expect(screen.getByTestId("nav-settings")).toBeInTheDocument();
  });

  it("hides the active team pill even when teams are active", () => {
    mockState = {
      activeTeams: {
        "ctx-1": { teammates: { "t-1": {}, "t-2": {} } },
      },
    };

    render(<Navigation {...defaultProps} hideViews />);

    // Team count chip should not render
    expect(screen.queryByText("2")).toBeNull();
  });

  it("renders all view items when hideViews is false (default)", () => {
    render(<Navigation {...defaultProps} />);

    expect(screen.getByTestId("nav-agents")).toBeInTheDocument();
    expect(screen.getByTestId("nav-settings")).toBeInTheDocument();
  });
});
