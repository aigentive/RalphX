import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { AppTopBar } from "./AppTopBar";
import { useProjectStore } from "@/stores/projectStore";
import type { Project } from "@/types/project";

// ProjectSelector and ThemeSelector are heavy children; stub them so the test
// focuses on AppTopBar's breadcrumb + project-selector gating logic.
vi.mock("@/components/projects/ProjectSelector", () => ({
  ProjectSelector: () => (
    <div data-testid="project-selector-stub">Project Selector</div>
  ),
}));

vi.mock("./ThemeSelector", () => ({
  ThemeSelector: () => <div data-testid="theme-selector-stub">Theme Selector</div>,
}));

vi.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: ReactNode }) => <span>{children}</span>,
}));

const project: Project = {
  id: "project-1",
  name: "RalphX",
  workingDirectory: "/tmp/ralphx",
  createdAt: "2024-01-01T00:00:00Z",
  updatedAt: "2024-01-01T00:00:00Z",
} as Project;

function renderTopBar(overrides: Partial<Parameters<typeof AppTopBar>[0]> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <AppTopBar
        currentView="ticketing"
        pendingReviewCount={0}
        reviewsPanelOpen={false}
        onToggleReviewsPanel={vi.fn()}
        {...overrides}
      />
    </QueryClientProvider>,
  );
}

describe("AppTopBar (ticketing view)", () => {
  beforeEach(() => {
    useProjectStore.setState({
      activeProjectId: "project-1",
      projects: { "project-1": project },
    } as never);
  });

  afterEach(() => {
    vi.clearAllMocks();
    useProjectStore.setState({ activeProjectId: null, projects: {} } as never);
  });

  it("renders the Ticketing breadcrumb with the project name", () => {
    renderTopBar({ currentView: "ticketing" });

    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb).toHaveTextContent("Workspace");
    expect(breadcrumb).toHaveTextContent("RalphX");
    expect(breadcrumb).toHaveTextContent("Ticketing");
  });

  it("falls back to 'Project' in the ticketing breadcrumb when no project is active", () => {
    useProjectStore.setState({ activeProjectId: null, projects: {} } as never);

    renderTopBar({ currentView: "ticketing" });

    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb).toHaveTextContent("Project");
    expect(breadcrumb).toHaveTextContent("Ticketing");
  });

  it("shows the project selector on the ticketing view when enabled", () => {
    renderTopBar({
      currentView: "ticketing",
      showProjectSelector: true,
      onNewProject: vi.fn(),
    });

    expect(screen.getByTestId("project-selector-stub")).toBeInTheDocument();
  });

  it("hides the project selector on views outside the allowlist", () => {
    renderTopBar({
      currentView: "activity",
      showProjectSelector: true,
      onNewProject: vi.fn(),
    });

    expect(screen.queryByTestId("project-selector-stub")).not.toBeInTheDocument();
    // Non-ticketing, non-kanban views fall back to the two-item workspace crumb.
    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb).toHaveTextContent("Activity");
    expect(breadcrumb).not.toHaveTextContent("Ticketing");
  });
});
