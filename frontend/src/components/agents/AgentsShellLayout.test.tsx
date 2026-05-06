import { createRef } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AgentsShellLayout } from "./AgentsShellLayout";

vi.mock("./AgentsSidebar", () => ({
  AgentsSidebar: () => <div data-testid="mock-agents-sidebar" />,
}));

vi.mock("./AgentsSidebarVisibilityProvider", () => ({
  AgentsSidebarVisibilityProvider: ({
    children,
  }: {
    children: React.ReactNode;
  }) => <div data-testid="mock-sidebar-visibility-provider">{children}</div>,
}));

vi.mock("@/components/ui/tooltip", () => ({
  TooltipProvider: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="mock-tooltip-provider">{children}</div>
  ),
}));

type ShellProps = React.ComponentProps<typeof AgentsShellLayout>;

function buildProps(overrides: Partial<ShellProps> = {}): ShellProps {
  const splitContainerRef = createRef<HTMLDivElement>();
  const sidebarProps = {} as ShellProps["sidebarProps"];
  return {
    isSidebarCollapsed: false,
    isSidebarOverlayOpen: false,
    onCloseSidebarOverlay: vi.fn(),
    onToggleSidebarCollapse: vi.fn(),
    sidebarProps,
    sidebarWidth: 280,
    splitContainerRef,
    suppressSidebarTransition: { current: false },
    children: <div data-testid="shell-children">children</div>,
    ...overrides,
  };
}

describe("AgentsShellLayout", () => {
  it("paints the section with the app content background and renders children", () => {
    render(<AgentsShellLayout {...buildProps()} />);

    const section = screen.getByTestId("agents-view");
    expect(section.style.backgroundColor).toBe("var(--app-content-bg)");

    expect(screen.getByTestId("shell-children")).toBeInTheDocument();
    expect(screen.getByTestId("mock-agents-sidebar")).toBeInTheDocument();

    const splitContainer = screen.getByTestId("agents-split-container");
    expect(splitContainer.style.backgroundColor).toBe("var(--app-content-bg)");
  });

  it("uses an animated transition when suppressSidebarTransition is false", () => {
    render(<AgentsShellLayout {...buildProps()} />);

    const sidebarContainer = screen.getByTestId("agents-sidebar-container");
    expect(sidebarContainer.style.transition).toBe("width 300ms ease");
  });

  it("disables the sidebar transition when suppressSidebarTransition.current is true", () => {
    render(
      <AgentsShellLayout
        {...buildProps({ suppressSidebarTransition: { current: true } })}
      />
    );

    const sidebarContainer = screen.getByTestId("agents-sidebar-container");
    expect(sidebarContainer.style.transition).toBe("none");
  });

  it("shows the toggle strip and triggers toggle on click + Enter/Space when collapsed", () => {
    const onToggle = vi.fn();
    render(
      <AgentsShellLayout
        {...buildProps({
          isSidebarCollapsed: true,
          onToggleSidebarCollapse: onToggle,
        })}
      />
    );

    const strip = screen.getByTestId("agents-sidebar-toggle-strip");
    expect(strip).toBeInTheDocument();

    fireEvent.click(strip);
    expect(onToggle).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(strip, { key: "Enter" });
    expect(onToggle).toHaveBeenCalledTimes(2);

    fireEvent.keyDown(strip, { key: " " });
    expect(onToggle).toHaveBeenCalledTimes(3);

    fireEvent.keyDown(strip, { key: "Tab" });
    expect(onToggle).toHaveBeenCalledTimes(3);

    fireEvent.mouseEnter(strip);
    expect(strip.style.backgroundColor).toBe("var(--overlay-weak)");

    fireEvent.mouseLeave(strip);
    expect(strip.style.backgroundColor).toBe("var(--app-sidebar-bg)");
  });

  it("renders the overlay backdrop and floating sidebar when overlay is open", () => {
    const onClose = vi.fn();
    render(
      <AgentsShellLayout
        {...buildProps({
          isSidebarOverlayOpen: true,
          onCloseSidebarOverlay: onClose,
        })}
      />
    );

    const backdrop = screen.getByTestId("agents-sidebar-overlay-backdrop");
    expect(backdrop).toBeInTheDocument();

    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);

    expect(
      screen.queryByTestId("agents-sidebar-container")
    ).not.toBeInTheDocument();

    expect(screen.getByTestId("mock-agents-sidebar")).toBeInTheDocument();
  });
});
