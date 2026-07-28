import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { AppTopBar } from "./AppTopBar";
import { notificationsApi } from "@/api/notifications";
import { useProjectStore } from "@/stores/projectStore";
import type { Project } from "@/types/project";

vi.mock("@/api/notifications", () => ({
  notificationsApi: { setDockBadgeCount: vi.fn().mockResolvedValue(undefined) },
}));

// ProjectSelector and ThemeSelector are heavy children; stub them so the test
// focuses on AppTopBar's breadcrumb + project-selector gating logic.
vi.mock("@/components/projects/ProjectSelector", () => ({
  ProjectSelector: ({
    open,
    onOpenChange,
  }: {
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
  }) => (
    <button
      data-testid="project-selector-stub"
      aria-expanded={open}
      onClick={() => onOpenChange?.(!open)}
    >
      Project Selector
    </button>
  ),
}));

vi.mock("./EnvironmentSwitcher", () => ({
  EnvironmentSwitcher: ({
    open,
    onOpenChange,
  }: {
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
  }) => (
    <button
      data-testid="environment-switcher-stub"
      aria-expanded={open}
      onClick={() => onOpenChange?.(!open)}
    >
      Environment Switcher
    </button>
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
        attentionCount={0}
        notificationsPanelOpen={false}
        onToggleNotificationsPanel={vi.fn()}
        {...overrides}
      />
    </QueryClientProvider>,
  );
}

describe("AppTopBar (ticketing, GitHub, and Granola views)", () => {
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

  it("renders the GitHub breadcrumb with the project name", () => {
    renderTopBar({ currentView: "github" });

    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb).toHaveTextContent("Workspace");
    expect(breadcrumb).toHaveTextContent("RalphX");
    expect(breadcrumb).toHaveTextContent("GitHub");
  });

  it("renders the Granola breadcrumb with the project name", () => {
    renderTopBar({ currentView: "granola" });

    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb).toHaveTextContent("Workspace");
    expect(breadcrumb).toHaveTextContent("RalphX");
    expect(breadcrumb).toHaveTextContent("Granola");
  });

  it("falls back to 'Project' in the ticketing breadcrumb when no project is active", () => {
    useProjectStore.setState({ activeProjectId: null, projects: {} } as never);

    renderTopBar({ currentView: "ticketing" });

    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb).toHaveTextContent("Project");
    expect(breadcrumb).toHaveTextContent("Ticketing");
  });

  it("uses the combined attention and unread-history count with the existing badge testid", () => {
    renderTopBar({ attentionCount: 12, unreadNotificationCount: 3 });
    expect(screen.getByTestId("reviews-badge")).toHaveTextContent("9+");
    expect(screen.getByRole("button", { name: /notifications.*15/i })).toBeInTheDocument();
    expect(document.getElementById("notifications-toggle")).toBe(screen.getByTestId("reviews-toggle"));
  });

  it("syncs the dock badge from the same combined count without duplicate writes", () => {
    vi.mocked(notificationsApi.setDockBadgeCount).mockReturnValue(new Promise<null>(() => {}));
    const { rerender } = renderTopBar({ attentionCount: 17, unreadNotificationCount: 4 });

    expect(screen.getByTestId("reviews-badge")).toHaveTextContent("9+");
    expect(notificationsApi.setDockBadgeCount).toHaveBeenCalledWith(21);

    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <AppTopBar
          currentView="ticketing"
          attentionCount={17}
          unreadNotificationCount={4}
          notificationsPanelOpen={false}
          onToggleNotificationsPanel={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(notificationsApi.setDockBadgeCount).toHaveBeenCalledTimes(1);

    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <AppTopBar
          currentView="ticketing"
          attentionCount={17}
          unreadNotificationCount={0}
          notificationsPanelOpen={false}
          onToggleNotificationsPanel={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(notificationsApi.setDockBadgeCount).toHaveBeenLastCalledWith(17);
    expect(screen.getByTestId("reviews-badge")).toHaveTextContent("9+");
  });

  it("renders unread history in the numeric badge instead of a secondary dot", () => {
    renderTopBar({ attentionCount: 0, unreadNotificationCount: 3 });

    expect(screen.queryByTestId("notifications-unread-dot")).not.toBeInTheDocument();
    expect(screen.getByTestId("reviews-badge")).toHaveTextContent("3");
  });

  it("shows the project selector on the ticketing view when enabled", () => {
    renderTopBar({
      currentView: "ticketing",
      showProjectSelector: true,
      onNewProject: vi.fn(),
    });

    expect(screen.getByTestId("project-selector-stub")).toBeInTheDocument();
  });

  it("places environment immediately before project and keeps one chrome menu open", async () => {
    renderTopBar({
      currentView: "ticketing",
      showProjectSelector: true,
      onNewProject: vi.fn(),
    });
    const environment = screen.getByTestId("environment-switcher-stub");
    const projectSelector = screen.getByTestId("project-selector-stub");

    expect(
      environment.compareDocumentPosition(projectSelector) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    await userEvent.click(environment);
    expect(environment).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(projectSelector);
    expect(environment).toHaveAttribute("aria-expanded", "false");
    expect(projectSelector).toHaveAttribute("aria-expanded", "true");
  });

  it("shows the project selector on the GitHub view when enabled", () => {
    renderTopBar({
      currentView: "github",
      showProjectSelector: true,
      onNewProject: vi.fn(),
    });

    expect(screen.getByTestId("project-selector-stub")).toBeInTheDocument();
  });

  it("shows the project selector on the Granola view when enabled", () => {
    renderTopBar({
      currentView: "granola",
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
    // Non-project-scoped views fall back to the two-item workspace crumb.
    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb).toHaveTextContent("Activity");
    expect(breadcrumb).not.toHaveTextContent("Ticketing");
  });
});
